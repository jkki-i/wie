use std::{collections::HashMap, fmt::Display, mem::size_of};

use crate::{
    core::arm::{allocator::Allocator, ArmCore},
    wipi::java::{get_all_java_classes, get_array_proto, JavaBridge, JavaClassProto, JavaError, JavaMethodBody, JavaObjectProxy, JavaResult},
};

use super::super::Context;

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClass {
    ptr_next: u32,
    unk1: u32,
    ptr_descriptor: u32,
    unk2: u32,
    unk3: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClassDescriptor {
    ptr_name: u32,
    unk1: u32,
    parent_class: u32,
    ptr_methods: u32,
    ptr_interfaces: u32,
    ptr_properties: u32,
    method_count: u16,
    fields_size: u16,
    access_flag: u16,
    unk6: u16,
    unk7: u16,
    index: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaMethod {
    fn_body: u32,
    ptr_class: u32,
    unk1: u32,
    ptr_name: u32,
    unk2: u16,
    unk3: u16,
    vtable_index: u16,
    access_flag: u16,
    unk6: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClassInstance {
    ptr_fields: u32,
    ptr_class: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClassInstanceFields {
    vtable_index: u32, // left shifted by 5
    fields: [u32; 1],
}

#[derive(Clone)]
pub struct JavaMethodFullname {
    pub tag: u8,
    pub name: String,
    pub signature: String,
}

impl JavaMethodFullname {
    pub fn from_ptr(core: &ArmCore, ptr: u32) -> JavaResult<Self> {
        let tag = core.read(ptr)?;

        let value = core.read_null_terminated_string(ptr + 1)?;
        let value = value.split('+').collect::<Vec<_>>();

        Ok(JavaMethodFullname {
            tag,
            name: value[1].into(),
            signature: value[0].into(),
        })
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        bytes.push(self.tag);
        bytes.extend_from_slice(self.signature.as_bytes());
        bytes.push(b'+');
        bytes.extend_from_slice(self.name.as_bytes());
        bytes.push(0);

        bytes
    }
}

impl Display for JavaMethodFullname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)?;
        self.signature.fmt(f)?;
        write!(f, "@{}", self.tag)?;

        Ok(())
    }
}

impl PartialEq for JavaMethodFullname {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.name == other.name
    }
}

pub struct JavaBridgeContext {
    loaded_classes: HashMap<String, u32>,
}

impl JavaBridgeContext {
    pub fn new() -> Self {
        Self {
            loaded_classes: HashMap::new(),
        }
    }
}

pub struct KtfJavaBridge<'a> {
    core: &'a mut ArmCore,
    context: &'a Context,
}

impl<'a> KtfJavaBridge<'a> {
    pub fn new(core: &'a mut ArmCore, context: &'a Context) -> Self {
        Self { core, context }
    }

    pub fn get_method(&mut self, ptr_class: u32, fullname: JavaMethodFullname) -> JavaResult<u32> {
        let class = self.core.read::<JavaClass>(ptr_class)?;
        let class_descriptor = self.core.read::<JavaClassDescriptor>(class.ptr_descriptor)?;
        let class_name = self.core.read_null_terminated_string(class_descriptor.ptr_name)?;

        let mut cursor = class_descriptor.ptr_methods;
        loop {
            let ptr = self.core.read::<u32>(cursor)?;
            if ptr == 0 {
                log::error!("Can't find function {} from {}", fullname, class_name);

                return Ok(0);
            }

            let current_method = self.core.read::<JavaMethod>(ptr)?;
            let current_fullname = JavaMethodFullname::from_ptr(self.core, current_method.ptr_name)?;

            if current_fullname == fullname {
                return Ok(ptr);
            }

            cursor += 4;
        }
    }

    pub fn load_class(&mut self, ptr_target: u32, name: &str) -> JavaResult<()> {
        let ptr_class = self.get_ptr_class(name)?;

        self.core.write(ptr_target, ptr_class)?;

        Ok(())
    }

    pub fn instantiate_from_ptr_class(&mut self, ptr_class: u32) -> JavaResult<JavaObjectProxy> {
        let class = self.core.read::<JavaClass>(ptr_class)?;
        let class_descriptor = self.core.read::<JavaClassDescriptor>(class.ptr_descriptor)?;
        let class_name = self.core.read_null_terminated_string(class_descriptor.ptr_name)?;

        let proxy = self.instantiate_inner(ptr_class, class_descriptor.fields_size as u32, ((class_descriptor.index * 4) as u32) << 5)?;

        log::info!("Instantiated {} at {:#x}", class_name, proxy.ptr_instance);

        Ok(proxy)
    }

    pub fn load_all_classes(&mut self) -> JavaResult<Vec<u32>> {
        let all_classes = get_all_java_classes();

        let loaded_classes = all_classes
            .into_iter()
            .enumerate()
            .map(|(index, (name, proto))| {
                let ptr_class = self.load_class_into_vm(index, name, proto)?;

                Ok((name.into(), ptr_class))
            })
            .collect::<JavaResult<Vec<_>>>()?;

        self.context
            .borrow_mut()
            .java_bridge_context
            .loaded_classes
            .extend(loaded_classes.iter().cloned());

        Ok(loaded_classes.into_iter().map(|x| x.1).collect())
    }

    pub fn get_ptr_methods(&self, ptr_class: u32) -> anyhow::Result<u32> {
        let class = self.core.read::<JavaClass>(ptr_class)?;
        let class_descriptor = self.core.read::<JavaClassDescriptor>(class.ptr_descriptor)?;

        Ok(class_descriptor.ptr_methods)
    }

    fn instantiate_inner(&mut self, ptr_class: u32, fields_size: u32, index: u32) -> JavaResult<JavaObjectProxy> {
        let ptr_instance = Allocator::alloc(self.core, size_of::<JavaClassInstance>() as u32)?;
        let ptr_fields = Allocator::alloc(self.core, fields_size + 4)?;

        self.core.write(ptr_instance, JavaClassInstance { ptr_fields, ptr_class })?;
        self.core.write(ptr_fields, index)?;

        Ok(JavaObjectProxy::new(ptr_instance))
    }

    fn get_ptr_class(&mut self, name: &str) -> JavaResult<u32> {
        let loaded_class = self.context.borrow_mut().java_bridge_context.loaded_classes.get(name).cloned();

        if let Some(loaded_class) = loaded_class {
            Ok(loaded_class)
        } else {
            // array class is created dynamically
            if name.as_bytes()[0] == b'[' {
                let ptr_class = self.load_class_into_vm(0, name, get_array_proto())?;

                self.context
                    .borrow_mut()
                    .java_bridge_context
                    .loaded_classes
                    .insert(name.into(), ptr_class);

                Ok(ptr_class)
            } else {
                Err(anyhow::anyhow!("No such class {}", name))
            }
        }
    }

    fn load_class_into_vm(&mut self, index: usize, name: &str, proto: JavaClassProto) -> JavaResult<u32> {
        let ptr_class = Allocator::alloc(self.core, size_of::<JavaClass>() as u32)?;
        self.core.write(
            ptr_class,
            JavaClass {
                ptr_next: ptr_class + 4,
                unk1: 0,
                ptr_descriptor: 0,
                unk2: 0,
                unk3: 0,
            },
        )?;

        let method_count = proto.methods.len();
        let ptr_methods = Allocator::alloc(self.core, ((method_count + 1) * size_of::<u32>()) as u32)?;

        let mut cursor = ptr_methods;
        for (index, method) in proto.methods.into_iter().enumerate() {
            let fullname = (JavaMethodFullname {
                tag: 0,
                name: method.name,
                signature: method.signature,
            })
            .as_bytes();

            let ptr_name = Allocator::alloc(self.core, fullname.len() as u32)?;
            self.core.write_raw(ptr_name, &fullname)?;

            let ptr_method = Allocator::alloc(self.core, size_of::<JavaMethod>() as u32)?;
            let fn_body = self.register_java_method(method.body)?;
            self.core.write(
                ptr_method,
                JavaMethod {
                    fn_body,
                    ptr_class,
                    unk1: 0,
                    ptr_name,
                    unk2: 0,
                    unk3: 0,
                    vtable_index: index as u16,
                    access_flag: 1, //  ACC_PUBLIC
                    unk6: 0,
                },
            )?;

            self.core.write(cursor, ptr_method)?;
            cursor += 4;
        }

        let ptr_name = Allocator::alloc(self.core, (name.len() + 1) as u32)?;
        self.core.write_raw(ptr_name, name.as_bytes())?;

        let ptr_descriptor = Allocator::alloc(self.core, size_of::<JavaClassDescriptor>() as u32)?;
        self.core.write(
            ptr_descriptor,
            JavaClassDescriptor {
                ptr_name,
                unk1: 0,
                parent_class: 0,
                ptr_methods,
                ptr_interfaces: 0,
                ptr_properties: 0,
                method_count: method_count as u16,
                fields_size: 0,
                access_flag: 0x21, // ACC_PUBLIC | ACC_SUPER
                unk6: 0,
                unk7: 0,
                index: index as u16,
            },
        )?;

        self.core.write(ptr_class + 8, ptr_descriptor)?;

        Ok(ptr_class)
    }

    fn register_java_method(&mut self, body: Box<dyn JavaMethodBody<JavaError>>) -> JavaResult<u32> {
        let closure = move |core: &mut ArmCore, context: &Context, a0: u32, a1: u32, a2: u32| {
            let mut java_bridge = KtfJavaBridge::new(core, context);
            let result = body.call(&mut java_bridge, vec![a0, a1, a2])?; // TODO do we need arg proxy?

            Ok::<_, JavaError>(result)
        };

        self.core.register_function(closure, self.context)
    }
}

impl JavaBridge for KtfJavaBridge<'_> {
    fn instantiate(&mut self, type_name: &str) -> JavaResult<JavaObjectProxy> {
        if type_name.as_bytes()[0] == b'[' {
            return Err(anyhow::anyhow!("Array class should not be instantiated here"));
        }
        let class_name = &type_name[1..type_name.len() - 1]; // L{};
        let ptr_class = self.get_ptr_class(class_name)?;

        self.instantiate_from_ptr_class(ptr_class)
    }

    fn instantiate_array(&mut self, element_type_name: &str, count: u32) -> JavaResult<JavaObjectProxy> {
        let array_type = format!("[{}", element_type_name);
        let ptr_class = self.get_ptr_class(&array_type)?;

        let proxy = self.instantiate_inner(ptr_class, count * 4 + 4, 0)?;
        let instance = self.core.read::<JavaClassInstance>(proxy.ptr_instance)?;
        self.core.write(instance.ptr_fields + 4, count)?;

        log::info!("Instantiated {} at {:#x}", array_type, proxy.ptr_instance);

        Ok(proxy)
    }

    fn call_method(&mut self, instance_proxy: &JavaObjectProxy, name: &str, signature: &str, args: &[u32]) -> JavaResult<u32> {
        let instance = self.core.read::<JavaClassInstance>(instance_proxy.ptr_instance)?;
        let class = self.core.read::<JavaClass>(instance.ptr_class)?;
        let class_descriptor = self.core.read::<JavaClassDescriptor>(class.ptr_descriptor)?;
        let class_name = self.core.read_null_terminated_string(class_descriptor.ptr_name)?;

        log::info!("Call {}::{}({})", class_name, name, signature);

        let fullname = JavaMethodFullname {
            tag: 0,
            name: name.to_owned(),
            signature: signature.to_owned(),
        };

        let ptr_method = self.get_method(instance.ptr_class, fullname)?;

        let method = self.core.read::<JavaMethod>(ptr_method)?;

        let mut params = vec![0, instance_proxy.ptr_instance];
        if !args.is_empty() {
            params.push(args[0]);
        }
        if args.len() > 1 {
            params.push(args[1]);
        }

        self.core.run_function(method.fn_body, &params)
    }

    fn get_field(&mut self, _instance_proxy: &JavaObjectProxy, _field_offset: u32) -> JavaResult<u32> {
        todo!()
    }

    fn put_field(&mut self, _instance_proxy: &JavaObjectProxy, _field_offset: u32, _value: u32) {
        todo!()
    }
}