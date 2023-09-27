use alloc::{borrow::ToOwned, boxed::Box, collections::BTreeMap, format, rc::Rc, string::String, vec::Vec};
use core::{cell::RefCell, fmt::Debug};

use capstone::{arch::BuildsCapstone, Capstone};
use unicorn_engine::{
    unicorn_const::{uc_error, Arch, HookType, MemType, Mode, Permission},
    RegisterARM, Unicorn,
};

use wie_backend::{task, AsyncCallable};
use wie_base::util::{read_generic, round_up, ByteRead, ByteWrite};

use crate::{
    context::ArmCoreContext,
    function::{EmulatedFunction, RegisteredFunction, RegisteredFunctionHolder, ResultWriter},
    future::SpawnFuture,
};

const IMAGE_BASE: u32 = 0x100000;
const FUNCTIONS_BASE: u32 = 0x71000000;
pub const RUN_FUNCTION_LR: u32 = 0x7f000000;
pub const HEAP_BASE: u32 = 0x40000000;
pub const PEB_BASE: u32 = 0x7ff00000;

#[derive(Debug)]
pub struct UnicornError(uc_error);

impl From<UnicornError> for anyhow::Error {
    fn from(err: UnicornError) -> Self {
        anyhow::anyhow!("{:?}", err.0)
    }
}

pub type ArmCoreError = anyhow::Error;
pub type ArmCoreResult<T> = anyhow::Result<T>;

struct ArmCoreInner {
    uc: Unicorn<'static, ()>,
    functions: BTreeMap<u32, Rc<Box<dyn RegisteredFunction>>>,
    functions_count: usize,
}

#[derive(Clone)]
pub struct ArmCore {
    inner: Rc<RefCell<ArmCoreInner>>,
}

impl ArmCore {
    pub fn new() -> ArmCoreResult<Self> {
        let mut uc = Unicorn::new(Arch::ARM, Mode::LITTLE_ENDIAN).map_err(UnicornError)?;

        // uc.add_block_hook(Self::code_hook).map_err(UnicornError)?;
        uc.add_mem_hook(HookType::MEM_INVALID, 0, 0xffff_ffff_ffff_ffff, Self::mem_hook)
            .map_err(UnicornError)?;

        uc.mem_map(FUNCTIONS_BASE as u64, 0x1000, Permission::READ | Permission::EXEC)
            .map_err(UnicornError)?;
        uc.add_code_hook(FUNCTIONS_BASE as u64, FUNCTIONS_BASE as u64 + 0x1000, |uc, _, _| uc.emu_stop().unwrap())
            .map_err(UnicornError)?;

        uc.reg_write(RegisterARM::CPSR, 0x40000010).map_err(UnicornError)?; // usr32

        let inner = ArmCoreInner {
            uc,
            functions: BTreeMap::new(),
            functions_count: 0,
        };

        Ok(Self {
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    pub fn load(&mut self, data: &[u8], map_size: usize) -> ArmCoreResult<u32> {
        let mut inner = self.inner.borrow_mut();

        inner
            .uc
            .mem_map(IMAGE_BASE as u64, round_up(map_size, 0x1000), Permission::ALL)
            .map_err(UnicornError)?;
        inner.uc.mem_write(IMAGE_BASE as u64, data).map_err(UnicornError)?;

        Ok(IMAGE_BASE)
    }

    #[allow(clippy::await_holding_refcell_ref)] // We manually drop RefMut https://github.com/rust-lang/rust-clippy/issues/6353
    async fn run_some(&mut self) -> ArmCoreResult<()> {
        let mut inner = self.inner.borrow_mut();

        let pc = inner.uc.reg_read(RegisterARM::PC).map_err(UnicornError)? as u32 + 1;
        inner.uc.emu_start(pc as u64, RUN_FUNCTION_LR as u64, 0, 0).map_err(UnicornError)?;

        let cur_pc = inner.uc.reg_read(RegisterARM::PC).map_err(UnicornError)? as u32;

        if (FUNCTIONS_BASE..FUNCTIONS_BASE + 0x1000).contains(&cur_pc) {
            let mut self1 = self.clone();

            let function = inner.functions.get(&cur_pc).unwrap().clone();

            drop(inner);

            function.call(&mut self1).await?;
        }

        Ok(())
    }

    #[allow(clippy::await_holding_refcell_ref)] // We manually drop RefMut https://github.com/rust-lang/rust-clippy/issues/6353
    pub async fn run_function<R>(&mut self, address: u32, params: &[u32]) -> ArmCoreResult<R>
    where
        R: RunFunctionResult<R>,
    {
        let previous_context = self.save_context(); // do we have to save context?
        let mut inner = self.inner.borrow_mut();

        if !params.is_empty() {
            inner.uc.reg_write(RegisterARM::R0, params[0] as u64).map_err(UnicornError)?;
        }
        if params.len() > 1 {
            inner.uc.reg_write(RegisterARM::R1, params[1] as u64).map_err(UnicornError)?;
        }
        if params.len() > 2 {
            inner.uc.reg_write(RegisterARM::R2, params[2] as u64).map_err(UnicornError)?;
        }
        if params.len() > 3 {
            inner.uc.reg_write(RegisterARM::R3, params[3] as u64).map_err(UnicornError)?;
        }
        if params.len() > 4 {
            for param in params[4..].iter() {
                let sp = inner.uc.reg_read(RegisterARM::SP).map_err(UnicornError)? as u32 - 4;
                inner.uc.mem_write(sp as u64, &param.to_le_bytes()).map_err(UnicornError)?;
                inner.uc.reg_write(RegisterARM::SP, sp as u64).map_err(UnicornError)?;
            }
        }

        inner.uc.reg_write(RegisterARM::PC, address as u64).map_err(UnicornError)?;
        inner.uc.reg_write(RegisterARM::LR, RUN_FUNCTION_LR as u64).map_err(UnicornError)?;
        drop(inner);

        loop {
            let (pc, _) = self.read_pc_lr().unwrap();
            if pc == RUN_FUNCTION_LR {
                break;
            }

            self.run_some().await?;
        }

        let result = R::get(self);

        self.restore_context(&previous_context);

        Ok(result)
    }

    pub fn spawn<C, R, E>(&mut self, callable: C)
    where
        C: AsyncCallable<R, E> + 'static,
        R: 'static,
        E: Debug + 'static,
    {
        let self_cloned = self.clone();
        task::spawn(move || SpawnFuture::new(self_cloned, callable));
    }

    pub fn register_function<F, P, E, C, R>(&mut self, function: F, context: &C) -> ArmCoreResult<u32>
    where
        F: EmulatedFunction<P, E, C, R> + 'static,
        E: Debug + 'static,
        C: Clone + 'static,
        R: ResultWriter<R> + 'static,
        P: 'static,
    {
        let mut inner = self.inner.borrow_mut();

        let bytes = [0x70, 0x47]; // BX LR
        let address = FUNCTIONS_BASE as u64 + (inner.functions_count * 2) as u64;

        inner.uc.mem_write(address, &bytes).map_err(UnicornError)?;

        let callback = RegisteredFunctionHolder::new(function, context);

        inner.functions.insert(address as u32, Rc::new(Box::new(callback)));
        inner.functions_count += 1;

        tracing::trace!("Register function at {:#x}", address);

        Ok(address as u32 + 1)
    }

    pub fn map(&mut self, address: u32, size: u32) -> ArmCoreResult<()> {
        tracing::trace!("Map address: {:#x}, size: {:#x}", address, size);

        let mut inner = self.inner.borrow_mut();

        inner
            .uc
            .mem_map(address as u64, size as usize, Permission::READ | Permission::WRITE)
            .map_err(UnicornError)?;

        Ok(())
    }

    fn dump_regs(&self) -> ArmCoreResult<String> {
        let inner = self.inner.borrow();

        Self::dump_regs_inner(&inner.uc)
    }

    fn format_callstack_address(address: u32) -> String {
        let description = if (IMAGE_BASE..IMAGE_BASE + 0x100000).contains(&address) {
            format!("client.bin+{:#x}", address - IMAGE_BASE)
        } else if (FUNCTIONS_BASE..FUNCTIONS_BASE + 0x10000).contains(&address) {
            "<Native function>".to_owned()
        } else {
            "<Unknown>".to_owned()
        };

        format!("{:#x}: {}\n", address, description)
    }

    fn dump_call_stack(&self) -> ArmCoreResult<String> {
        let inner = self.inner.borrow();

        let sp = inner.uc.reg_read(RegisterARM::SP).map_err(UnicornError)?;
        let pc = inner.uc.reg_read(RegisterARM::PC).map_err(UnicornError)?;
        let lr = inner.uc.reg_read(RegisterARM::LR).map_err(UnicornError)?;

        let mut call_stack = Self::format_callstack_address(pc as u32);
        if lr as u32 != RUN_FUNCTION_LR && lr != 0 {
            call_stack += &Self::format_callstack_address((lr - 5) as u32);
        }

        for i in 0..128 {
            let address = sp + (i * 4);
            let value = inner.uc.mem_read_as_vec(address, 4).map_err(UnicornError)?;
            let value_u32 = u32::from_le_bytes(value.try_into().unwrap());

            if value_u32 % 2 == 1 {
                // TODO image size temp
                if (IMAGE_BASE..IMAGE_BASE + 0x100000).contains(&value_u32) {
                    call_stack += &Self::format_callstack_address(value_u32 - 5);
                }
            }
        }

        Ok(call_stack)
    }

    fn dump_stack(&self) -> ArmCoreResult<String> {
        let inner = self.inner.borrow();

        let sp = inner.uc.reg_read(RegisterARM::SP).map_err(UnicornError)?;

        let mut result = String::new();
        for i in 0..16 {
            let address = sp + (i * 4);
            let value = inner.uc.mem_read_as_vec(address, 4).map_err(UnicornError)?;
            let value_u32 = u32::from_le_bytes(value.try_into().unwrap());

            result += &format!("SP+{:#x}: {:#x}\n", i * 4, value_u32);
        }

        Ok(result)
    }

    pub(crate) fn read_pc_lr(&self) -> ArmCoreResult<(u32, u32)> {
        let inner = self.inner.borrow();

        let lr = inner.uc.reg_read(RegisterARM::LR).map_err(UnicornError)? as u32;
        let pc = inner.uc.reg_read(RegisterARM::PC).map_err(UnicornError)? as u32;

        Ok((pc, lr))
    }

    pub(crate) fn write_result(&mut self, result: u32, lr: u32) -> ArmCoreResult<()> {
        let mut inner = self.inner.borrow_mut();

        inner.uc.reg_write(RegisterARM::R0, result as u64).map_err(UnicornError)?;
        inner.uc.reg_write(RegisterARM::PC, lr as u64).map_err(UnicornError)?;

        Ok(())
    }

    pub(crate) fn read_param(&self, pos: usize) -> ArmCoreResult<u32> {
        let inner = self.inner.borrow();

        let result = if pos == 0 {
            inner.uc.reg_read(RegisterARM::R0).map_err(UnicornError)? as u32
        } else if pos == 1 {
            inner.uc.reg_read(RegisterARM::R1).map_err(UnicornError)? as u32
        } else if pos == 2 {
            inner.uc.reg_read(RegisterARM::R2).map_err(UnicornError)? as u32
        } else if pos == 3 {
            inner.uc.reg_read(RegisterARM::R3).map_err(UnicornError)? as u32
        } else {
            let sp = inner.uc.reg_read(RegisterARM::SP).map_err(UnicornError)? as u32;

            drop(inner);

            read_generic(self, sp + 4 * (pos as u32 - 4))?
        };

        Ok(result)
    }

    fn dump_regs_inner(uc: &Unicorn<'_, ()>) -> ArmCoreResult<String> {
        let value = (|| {
            Ok::<_, uc_error>(
                [
                    format!(
                        "R0: {:#x} R1: {:#x} R2: {:#x} R3: {:#x} R4: {:#x} R5: {:#x} R6: {:#x} R7: {:#x} R8: {:#x}",
                        uc.reg_read(RegisterARM::R0)?,
                        uc.reg_read(RegisterARM::R1)?,
                        uc.reg_read(RegisterARM::R2)?,
                        uc.reg_read(RegisterARM::R3)?,
                        uc.reg_read(RegisterARM::R4)?,
                        uc.reg_read(RegisterARM::R5)?,
                        uc.reg_read(RegisterARM::R6)?,
                        uc.reg_read(RegisterARM::R7)?,
                        uc.reg_read(RegisterARM::R8)?,
                    ),
                    format!(
                        "SB: {:#x} SL: {:#x} FP: {:#x} IP: {:#x} SP: {:#x} LR: {:#x} PC: {:#x}",
                        uc.reg_read(RegisterARM::SB)?,
                        uc.reg_read(RegisterARM::SL)?,
                        uc.reg_read(RegisterARM::FP)?,
                        uc.reg_read(RegisterARM::IP)?,
                        uc.reg_read(RegisterARM::SP)?,
                        uc.reg_read(RegisterARM::LR)?,
                        uc.reg_read(RegisterARM::PC)?,
                    ),
                    format!("APSR: {:032b}\n", uc.reg_read(RegisterARM::APSR)?),
                ]
                .join("\n"),
            )
        })()
        .map_err(UnicornError)?;

        Ok(value)
    }

    #[allow(dead_code)]
    #[allow(unknown_lints)]
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn code_hook(uc: &mut Unicorn<'_, ()>, address: u64, size: u32) {
        let insn = uc.mem_read_as_vec(address, size as usize).unwrap();

        let cs = Capstone::new()
            .arm()
            .mode(capstone::arch::arm::ArchMode::Thumb)
            .detail(true)
            .build()
            .unwrap();

        let insns = cs.disasm_all(&insn, address).unwrap();

        let insn_str = insns
            .iter()
            .map(|x| format!("{:#x}: {} {}", x.address(), x.mnemonic().unwrap(), x.op_str().unwrap()))
            .collect::<Vec<_>>()
            .join("\n");

        tracing::trace!("{}\n{}", insn_str, Self::dump_regs_inner(uc).unwrap());
    }

    #[allow(unknown_lints)]
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn mem_hook(uc: &mut Unicorn<'_, ()>, mem_type: MemType, address: u64, size: usize, value: i64) -> bool {
        let pc = uc.reg_read(RegisterARM::PC).unwrap();
        let lr = uc.reg_read(RegisterARM::LR).unwrap();

        if mem_type == MemType::FETCH_PROT && pc == address && (FUNCTIONS_BASE..FUNCTIONS_BASE + 0x1000).contains(&(address as u32)) {
            return false;
        }

        if mem_type == MemType::READ || mem_type == MemType::FETCH || mem_type == MemType::WRITE {
            let value_str = if mem_type == MemType::WRITE {
                format!("{:#x}", value)
            } else {
                let value = uc.mem_read_as_vec(address, size).unwrap();

                if size == 4 {
                    format!("{:#x}", u32::from_le_bytes(value.try_into().unwrap()))
                } else {
                    format!("{:?}", value)
                }
            };

            tracing::trace!(
                "pc: {:#x} lr: {:#x} mem_type: {:?} address: {:#x} size: {:#x} value: {}",
                pc,
                lr,
                mem_type,
                address,
                size,
                value_str
            );

            true
        } else {
            tracing::error!(
                "Invalid Memory Access\n\
                mem_type: {:?} address: {:#x} size: {:#x} value: {:#x}\n{}",
                mem_type,
                address,
                size,
                value,
                Self::dump_regs_inner(uc).unwrap()
            );

            false
        }
    }

    pub fn restore_context(&mut self, context: &ArmCoreContext) {
        let mut inner = self.inner.borrow_mut();

        inner.uc.reg_write(RegisterARM::R0, context.r0 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R1, context.r1 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R2, context.r2 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R3, context.r3 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R4, context.r4 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R5, context.r5 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R6, context.r6 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R7, context.r7 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::R8, context.r8 as u64).unwrap();
        inner.uc.reg_write(RegisterARM::SB, context.sb as u64).unwrap();
        inner.uc.reg_write(RegisterARM::SL, context.sl as u64).unwrap();
        inner.uc.reg_write(RegisterARM::FP, context.fp as u64).unwrap();
        inner.uc.reg_write(RegisterARM::IP, context.ip as u64).unwrap();
        inner.uc.reg_write(RegisterARM::SP, context.sp as u64).unwrap();
        inner.uc.reg_write(RegisterARM::LR, context.lr as u64).unwrap();
        inner.uc.reg_write(RegisterARM::PC, context.pc as u64).unwrap();
        inner.uc.reg_write(RegisterARM::APSR, context.apsr as u64).unwrap();
    }

    pub fn save_context(&self) -> ArmCoreContext {
        let inner = self.inner.borrow();

        ArmCoreContext {
            r0: inner.uc.reg_read(RegisterARM::R0).unwrap() as u32,
            r1: inner.uc.reg_read(RegisterARM::R1).unwrap() as u32,
            r2: inner.uc.reg_read(RegisterARM::R2).unwrap() as u32,
            r3: inner.uc.reg_read(RegisterARM::R3).unwrap() as u32,
            r4: inner.uc.reg_read(RegisterARM::R4).unwrap() as u32,
            r5: inner.uc.reg_read(RegisterARM::R5).unwrap() as u32,
            r6: inner.uc.reg_read(RegisterARM::R6).unwrap() as u32,
            r7: inner.uc.reg_read(RegisterARM::R7).unwrap() as u32,
            r8: inner.uc.reg_read(RegisterARM::R8).unwrap() as u32,
            sb: inner.uc.reg_read(RegisterARM::SB).unwrap() as u32,
            sl: inner.uc.reg_read(RegisterARM::SL).unwrap() as u32,
            fp: inner.uc.reg_read(RegisterARM::FP).unwrap() as u32,
            ip: inner.uc.reg_read(RegisterARM::IP).unwrap() as u32,
            sp: inner.uc.reg_read(RegisterARM::SP).unwrap() as u32,
            lr: inner.uc.reg_read(RegisterARM::LR).unwrap() as u32,
            pc: inner.uc.reg_read(RegisterARM::PC).unwrap() as u32,
            apsr: inner.uc.reg_read(RegisterARM::APSR).unwrap() as u32,
        }
    }

    pub fn dump_reg_stack(&self) -> String {
        format!(
            "\n{}\nPossible call stack:\n{}\nStack:\n{}",
            self.dump_regs().unwrap(),
            self.dump_call_stack().unwrap(),
            self.dump_stack().unwrap()
        )
    }
}

impl ByteRead for ArmCore {
    fn read_bytes(&self, address: u32, size: u32) -> anyhow::Result<Vec<u8>> {
        let inner = self.inner.borrow();

        let data = inner.uc.mem_read_as_vec(address as u64, size as usize).map_err(UnicornError)?;

        // tracing::trace!("Read address: {:#x}, data: {:02x?}", address, data);

        Ok(data)
    }
}

impl ByteWrite for ArmCore {
    fn write_bytes(&mut self, address: u32, data: &[u8]) -> anyhow::Result<()> {
        // tracing::trace!("Write address: {:#x}, data: {:02x?}", address, data);
        let mut inner = self.inner.borrow_mut();

        inner.uc.mem_write(address as u64, data).map_err(UnicornError)?;

        Ok(())
    }
}

pub trait RunFunctionResult<R> {
    fn get(core: &ArmCore) -> R;
}

impl RunFunctionResult<u32> for u32 {
    fn get(core: &ArmCore) -> u32 {
        core.read_param(0).unwrap()
    }
}

impl RunFunctionResult<()> for () {
    fn get(_: &ArmCore) {}
}