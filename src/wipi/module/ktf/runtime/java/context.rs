use std::{fmt::Display, mem::size_of};

use crate::{
    backend::Backend,
    core::arm::{allocator::Allocator, ArmCore, PEB_BASE},
    util::{read_generic, read_null_terminated_string, write_generic, ByteWrite},
    wipi::{
        java::{get_array_proto, get_class_proto, JavaClassProto, JavaContextBase, JavaError, JavaMethodBody, JavaObjectProxy, JavaResult},
        module::ktf::runtime::init::KtfPeb,
    },
};

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClass {
    ptr_next: u32,
    unk1: u32,
    ptr_descriptor: u32,
    ptr_vtable: u32,
    vtable_count: u16,
    unk2: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JavaClassDescriptor {
    ptr_name: u32,
    unk1: u32,
    ptr_parent_class: u32,
    ptr_methods: u32,
    ptr_interfaces: u32,
    ptr_fields: u32,
    method_count: u16,
    fields_size: u16,
    access_flag: u16,
    unk6: u16,
    unk7: u16,
    unk8: u16,
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
struct JavaField {
    unk1: u32,
    ptr_class: u32,
    ptr_name: u32,
    offset: u32,
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
pub struct JavaFullName {
    pub tag: u8,
    pub name: String,
    pub signature: String,
}

impl JavaFullName {
    pub fn from_ptr(core: &ArmCore, ptr: u32) -> JavaResult<Self> {
        let tag = read_generic(core, ptr)?;

        let value = read_null_terminated_string(core, ptr + 1)?;
        let value = value.split('+').collect::<Vec<_>>();

        Ok(JavaFullName {
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

impl Display for JavaFullName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name.fmt(f)?;
        self.signature.fmt(f)?;
        write!(f, "@{}", self.tag)?;

        Ok(())
    }
}

impl PartialEq for JavaFullName {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature && self.name == other.name
    }
}

pub struct KtfJavaContext {
    core: ArmCore,
    backend: Backend,
}

impl KtfJavaContext {
    pub fn init(core: &mut ArmCore) -> JavaResult<u32> {
        let java_classes_base = Allocator::alloc(core, 0x1000)?;

        Ok(java_classes_base)
    }

    pub fn new(core: ArmCore, backend: Backend) -> Self {
        Self { core, backend }
    }

    pub fn get_method(&mut self, ptr_class: u32, fullname: JavaFullName) -> JavaResult<u32> {
        let (_, class_descriptor, class_name) = self.read_ptr_class(ptr_class)?;

        let mut cursor = class_descriptor.ptr_methods;
        loop {
            let ptr = read_generic::<u32>(&self.core, cursor)?;
            if ptr == 0 {
                log::error!("Can't find function {} from {}", fullname, class_name);

                return Ok(0);
            }

            let current_method = read_generic::<JavaMethod>(&self.core, ptr)?;
            let current_fullname = JavaFullName::from_ptr(&self.core, current_method.ptr_name)?;

            if current_fullname == fullname {
                return Ok(ptr);
            }

            cursor += 4;
        }
    }

    pub fn load_class(&mut self, ptr_target: u32, name: &str) -> JavaResult<()> {
        let ptr_class = self.find_ptr_class(name)?;

        write_generic(&mut self.core, ptr_target, ptr_class)?;

        Ok(())
    }

    pub fn instantiate_from_ptr_class(&mut self, ptr_class: u32) -> JavaResult<JavaObjectProxy> {
        let (_, class_descriptor, class_name) = self.read_ptr_class(ptr_class)?;

        let proxy = self.instantiate_inner(ptr_class, class_descriptor.fields_size as u32)?;

        log::info!("Instantiated {} at {:#x}", class_name, proxy.ptr_instance);

        Ok(proxy)
    }

    pub fn instantiate_array_from_ptr_class(&mut self, ptr_class_array: u32, count: u32) -> JavaResult<JavaObjectProxy> {
        let (_, _, class_name) = self.read_ptr_class(ptr_class_array)?;

        let proxy = self.instantiate_array_inner(ptr_class_array, count * 4 + 4)?;

        log::info!("Instantiated {} at {:#x}", class_name, proxy.ptr_instance);

        Ok(proxy)
    }

    fn instantiate_inner(&mut self, ptr_class: u32, fields_size: u32) -> JavaResult<JavaObjectProxy> {
        let ptr_instance = Allocator::alloc(&mut self.core, size_of::<JavaClassInstance>() as u32)?;
        let ptr_fields = Allocator::alloc(&mut self.core, fields_size + 4)?;

        let vtable_index = self.get_vtable_index(ptr_class)?;

        write_generic(&mut self.core, ptr_instance, JavaClassInstance { ptr_fields, ptr_class })?;
        write_generic(&mut self.core, ptr_fields, (vtable_index * 4) << 5)?;

        log::trace!("Instantiate {:#x}, vtable_index {:#x}", ptr_instance, vtable_index);

        Ok(JavaObjectProxy::new(ptr_instance))
    }

    fn instantiate_array_inner(&mut self, ptr_class_array: u32, count: u32) -> JavaResult<JavaObjectProxy> {
        let proxy = self.instantiate_inner(ptr_class_array, count * 4 + 4)?;
        let instance = read_generic::<JavaClassInstance>(&self.core, proxy.ptr_instance)?;

        write_generic(&mut self.core, instance.ptr_fields + 4, count)?;

        Ok(proxy)
    }

    fn write_vtable(&mut self, ptr_class: u32) -> anyhow::Result<u32> {
        let vtable = self.build_vtable(ptr_class)?;

        let ptr_vtable = Allocator::alloc(&mut self.core, (vtable.len() + 1) as u32 * 4)?;
        let mut cursor = ptr_vtable;
        for item in vtable {
            write_generic(&mut self.core, cursor, item)?;
            cursor += 4;
        }

        Ok(ptr_vtable)
    }

    fn get_vtable_index(&mut self, ptr_class: u32) -> anyhow::Result<u32> {
        let (class, _, _) = self.read_ptr_class(ptr_class)?;

        let peb = read_generic::<KtfPeb>(&self.core, PEB_BASE)?;
        let mut cursor = peb.vtables_base;
        loop {
            let current_vtable = read_generic::<u32>(&self.core, cursor)?;
            if current_vtable == 0 {
                write_generic(&mut self.core, cursor, class.ptr_vtable)?;
                break;
            }
            if current_vtable == class.ptr_vtable {
                break;
            }

            cursor += 4;
        }
        let index = (cursor - peb.vtables_base) / 4;

        Ok(index)
    }

    fn build_vtable(&mut self, ptr_class: u32) -> anyhow::Result<Vec<u32>> {
        let mut class_hierarchy = self.read_class_hierarchy(ptr_class)?;
        class_hierarchy.reverse();

        let mut vtable = Vec::new();

        for ptr_class in class_hierarchy {
            let (_, class_descriptor, _) = self.read_ptr_class(ptr_class)?;

            let mut cursor = class_descriptor.ptr_methods;
            loop {
                let ptr_method = read_generic::<u32>(&self.core, cursor)?;
                if ptr_method == 0 {
                    break;
                }
                vtable.push(ptr_method);
                cursor += 4;
            }
        }

        Ok(vtable)
    }

    fn read_class_hierarchy(&mut self, ptr_class: u32) -> anyhow::Result<Vec<u32>> {
        let mut result = vec![ptr_class];

        let mut current_class = ptr_class;
        loop {
            let (_, class_descriptor, _) = self.read_ptr_class(current_class)?;

            if class_descriptor.ptr_parent_class != 0 {
                result.push(class_descriptor.ptr_parent_class);

                current_class = class_descriptor.ptr_parent_class;
            } else {
                break;
            }
        }

        Ok(result)
    }

    fn find_ptr_class(&mut self, name: &str) -> JavaResult<u32> {
        let peb = read_generic::<KtfPeb>(&self.core, PEB_BASE)?;
        let mut cursor = peb.java_classes_base;
        loop {
            let ptr_class = read_generic::<u32>(&self.core, cursor)?;
            if ptr_class == 0 {
                break;
            }

            let (_, _, class_name) = self.read_ptr_class(ptr_class)?;

            if class_name == name {
                return Ok(ptr_class);
            }

            cursor += 4;
        }

        // array class is created dynamically
        if name.as_bytes()[0] == b'[' {
            let ptr_class = self.load_class_into_vm(name, get_array_proto())?;

            Ok(ptr_class)
        } else {
            let proto = get_class_proto(name).ok_or_else(|| anyhow::anyhow!("No such class {}", name))?;

            self.load_class_into_vm(name, proto)
        }
    }

    fn load_class_into_vm(&mut self, name: &str, proto: JavaClassProto) -> JavaResult<u32> {
        let method_count = proto.methods.len();

        let ptr_class = Allocator::alloc(&mut self.core, size_of::<JavaClass>() as u32)?;
        write_generic(
            &mut self.core,
            ptr_class,
            JavaClass {
                ptr_next: ptr_class + 4,
                unk1: 0,
                ptr_descriptor: 0,
                ptr_vtable: 0,
                vtable_count: method_count as u16,
                unk2: 0,
            },
        )?;

        let ptr_methods = Allocator::alloc(&mut self.core, ((method_count + 1) * size_of::<u32>()) as u32)?;
        let mut cursor = ptr_methods;
        for (index, method) in proto.methods.into_iter().enumerate() {
            let full_name = (JavaFullName {
                tag: 0,
                name: method.name,
                signature: method.signature,
            })
            .as_bytes();

            let ptr_name = Allocator::alloc(&mut self.core, full_name.len() as u32)?;
            self.core.write_bytes(ptr_name, &full_name)?;

            let ptr_method = Allocator::alloc(&mut self.core, size_of::<JavaMethod>() as u32)?;
            let fn_body = self.register_java_method(method.body)?;
            write_generic(
                &mut self.core,
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

            write_generic(&mut self.core, cursor, ptr_method)?;
            cursor += 4;
        }

        let ptr_fields = Allocator::alloc(&mut self.core, ((method_count + 1) * size_of::<u32>()) as u32)?;
        let mut cursor = ptr_fields;
        for (index, field) in proto.fields.into_iter().enumerate() {
            let full_name = (JavaFullName {
                tag: 0,
                name: field.name,
                signature: field.signature,
            })
            .as_bytes();

            let ptr_name = Allocator::alloc(&mut self.core, full_name.len() as u32)?;
            self.core.write_bytes(ptr_name, &full_name)?;

            let ptr_field = Allocator::alloc(&mut self.core, size_of::<JavaField>() as u32)?;
            write_generic(
                &mut self.core,
                ptr_field,
                JavaField {
                    unk1: 0,
                    ptr_class,
                    ptr_name,
                    offset: (index as u32) * 4,
                },
            )?;

            write_generic(&mut self.core, cursor, ptr_field)?;
            cursor += 4;
        }

        let ptr_name = Allocator::alloc(&mut self.core, (name.len() + 1) as u32)?;
        self.core.write_bytes(ptr_name, name.as_bytes())?;

        let ptr_descriptor = Allocator::alloc(&mut self.core, size_of::<JavaClassDescriptor>() as u32)?;
        write_generic(
            &mut self.core,
            ptr_descriptor,
            JavaClassDescriptor {
                ptr_name,
                unk1: 0,
                ptr_parent_class: 0,
                ptr_methods,
                ptr_interfaces: 0,
                ptr_fields,
                method_count: method_count as u16,
                fields_size: 0,
                access_flag: 0x21, // ACC_PUBLIC | ACC_SUPER
                unk6: 0,
                unk7: 0,
                unk8: 0,
            },
        )?;

        write_generic(&mut self.core, ptr_class + 8, ptr_descriptor)?;

        let ptr_vtable = self.write_vtable(ptr_class)?;
        write_generic(&mut self.core, ptr_class + 12, ptr_vtable)?;

        let peb = read_generic::<KtfPeb>(&self.core, PEB_BASE)?;
        let mut cursor = peb.java_classes_base;
        loop {
            let current = read_generic::<u32>(&self.core, cursor)?;
            if current == 0 {
                write_generic(&mut self.core, cursor, ptr_class)?;
                break;
            }
            cursor += 4;
        }

        Ok(ptr_class)
    }

    fn register_java_method(&mut self, body: JavaMethodBody) -> JavaResult<u32> {
        let closure = move |core: ArmCore, backend: Backend, _: u32, a1: u32, a2: u32| {
            let mut context = KtfJavaContext::new(core, backend);

            let result = body.call(&mut context, vec![a1, a2])?; // TODO do we need arg proxy?

            Ok::<_, JavaError>(result)
        };

        self.core.register_function(closure, &self.backend)
    }

    fn read_ptr_class(&self, ptr_class: u32) -> JavaResult<(JavaClass, JavaClassDescriptor, String)> {
        let class = read_generic::<JavaClass>(&self.core, ptr_class)?;
        let class_descriptor = read_generic::<JavaClassDescriptor>(&self.core, class.ptr_descriptor)?;
        let class_name = read_null_terminated_string(&self.core, class_descriptor.ptr_name)?;

        Ok((class, class_descriptor, class_name))
    }
}

impl JavaContextBase for KtfJavaContext {
    fn instantiate(&mut self, type_name: &str) -> JavaResult<JavaObjectProxy> {
        if type_name.as_bytes()[0] == b'[' {
            return Err(anyhow::anyhow!("Array class should not be instantiated here"));
        }
        let class_name = &type_name[1..type_name.len() - 1]; // L{};
        let ptr_class = self.find_ptr_class(class_name)?;

        let (_, class_descriptor, _) = self.read_ptr_class(ptr_class)?;

        let proxy = self.instantiate_inner(ptr_class, class_descriptor.fields_size as u32)?;

        log::info!("Instantiated {} at {:#x}", class_name, proxy.ptr_instance);

        Ok(proxy)
    }

    fn instantiate_array(&mut self, element_type_name: &str, count: u32) -> JavaResult<JavaObjectProxy> {
        let array_type = format!("[{}", element_type_name);
        let ptr_class_array = self.find_ptr_class(&array_type)?;

        let proxy = self.instantiate_array_inner(ptr_class_array, count)?;

        log::info!("Instantiated {} at {:#x}", array_type, proxy.ptr_instance);

        Ok(proxy)
    }

    fn call_method(&mut self, instance_proxy: &JavaObjectProxy, name: &str, signature: &str, args: &[u32]) -> JavaResult<u32> {
        let instance = read_generic::<JavaClassInstance>(&self.core, instance_proxy.ptr_instance)?;
        let (_, _, class_name) = self.read_ptr_class(instance.ptr_class)?;

        log::info!("Call {}::{}({})", class_name, name, signature);

        let fullname = JavaFullName {
            tag: 0,
            name: name.to_owned(),
            signature: signature.to_owned(),
        };

        let ptr_method = self.get_method(instance.ptr_class, fullname)?;

        let method = read_generic::<JavaMethod>(&self.core, ptr_method)?;

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

    fn backend(&mut self) -> &mut Backend {
        &mut self.backend
    }
}