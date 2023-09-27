use alloc::vec::Vec;
use core::mem::size_of;

use bytemuck::{Pod, Zeroable};

use wie_backend::Backend;
use wie_base::util::write_generic;
use wie_core_arm::ArmCore;
use wie_wipi_c::{
    get_database_method_table, get_graphics_method_table, get_kernel_method_table, get_media_method_table, get_stub_method_table, CContext,
    CMethodBody,
};

use crate::runtime::c::context::KtfCContext;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WIPICInterface {
    interface_0: u32,
    interface_1: u32,
    interface_2: u32,
    interface_3: u32,
    interface_4: u32,
    interface_5: u32,
    interface_6: u32,
    interface_7: u32,
    interface_8: u32,
    interface_9: u32,
    interface_10: u32,
    interface_11: u32,
    interface_12: u32,
}

fn write_methods(context: &mut dyn CContext, methods: Vec<CMethodBody>) -> anyhow::Result<u32> {
    let address = context.alloc_raw((methods.len() * 4) as u32)?;

    let mut cursor = address;
    for method in methods {
        let address = context.register_function(method)?;

        write_generic(context, cursor, address)?;
        cursor += 4;
    }

    Ok(address)
}

pub fn get_wipic_knl_interface(core: &mut ArmCore, backend: &mut Backend) -> anyhow::Result<u32> {
    let kernel_methods = get_kernel_method_table(get_wipic_interfaces);

    let mut context = KtfCContext::new(core, backend);
    let address = write_methods(&mut context, kernel_methods)?;

    Ok(address)
}

async fn get_wipic_interfaces(context: &mut dyn CContext) -> anyhow::Result<u32> {
    tracing::trace!("get_wipic_interfaces");

    let interface_0 = write_methods(context, get_stub_method_table(0))?;
    let interface_1 = write_methods(context, get_stub_method_table(1))?;

    let graphics_methods = get_graphics_method_table();
    let interface_2 = write_methods(context, graphics_methods)?;

    let interface_3 = write_methods(context, get_stub_method_table(3))?;
    let interface_4 = write_methods(context, get_stub_method_table(4))?;
    let interface_5 = write_methods(context, get_stub_method_table(5))?;

    let database_methods = get_database_method_table();
    let interface_6 = write_methods(context, database_methods)?;

    let interface_7 = write_methods(context, get_stub_method_table(7))?;
    let interface_8 = write_methods(context, get_stub_method_table(8))?;

    let media_methods = get_media_method_table();
    let interface_9 = write_methods(context, media_methods)?;

    let interface_10 = write_methods(context, get_stub_method_table(10))?;
    let interface_11 = write_methods(context, get_stub_method_table(11))?;
    let interface_12 = write_methods(context, get_stub_method_table(12))?;

    let interface = WIPICInterface {
        interface_0,
        interface_1,
        interface_2,
        interface_3,
        interface_4,
        interface_5,
        interface_6,
        interface_7,
        interface_8,
        interface_9,
        interface_10,
        interface_11,
        interface_12,
    };

    let address = context.alloc_raw(size_of::<WIPICInterface>() as u32)?;

    write_generic(context, address, interface)?;

    Ok(address)
}