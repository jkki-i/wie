use alloc::{boxed::Box, string::String, vec::Vec};
use core::{iter, mem::size_of};

use bytemuck::{Pod, Zeroable};

use jvm::{ArrayClassInstance, ClassInstance, Field, JavaValue, JvmResult};

use wie_base::util::{read_generic, write_generic, ByteWrite};
use wie_core_arm::{Allocator, ArmCore};
use wie_impl_java::{JavaResult, JavaWord};

use crate::runtime::java::context::context_data::JavaContextData;

use super::{class::JavaClass, field::JavaField, value::JavaValueExt, KtfJvmWord};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RawJavaClassInstance {
    ptr_fields: u32,
    ptr_class: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RawJavaClassInstanceFields {
    vtable_index: u32, // left shifted by 5
    fields: [u32; 1],
}

pub struct JavaClassInstance {
    pub(crate) ptr_raw: u32,
    core: ArmCore,
}

impl JavaClassInstance {
    pub fn from_raw(ptr_raw: u32, core: &ArmCore) -> Self {
        Self { ptr_raw, core: core.clone() }
    }

    pub fn new(core: &mut ArmCore, class: &JavaClass) -> JavaResult<Self> {
        let field_size = class.field_size()?;

        let instance = Self::instantiate(core, class, field_size)?;

        tracing::trace!("Instantiated {} at {:#x}", class.name()?, instance.ptr_raw);

        Ok(instance)
    }

    pub fn destroy(mut self) -> JavaResult<()> {
        let raw = self.read_raw()?;

        Allocator::free(&mut self.core, raw.ptr_fields)?;
        Allocator::free(&mut self.core, self.ptr_raw)?;

        Ok(())
    }

    pub fn class(&self) -> JavaResult<JavaClass> {
        let raw = self.read_raw()?;

        Ok(JavaClass::from_raw(raw.ptr_class, &self.core))
    }

    pub fn read_field(&self, field: &JavaField) -> JavaResult<KtfJvmWord> {
        let offset = field.offset()?;

        let address = self.field_address(offset)?;

        let value: KtfJvmWord = read_generic(&self.core, address)?;

        Ok(value)
    }

    pub fn write_field(&mut self, field: &JavaField, value: KtfJvmWord) -> JavaResult<()> {
        let offset = field.offset()?;

        let address = self.field_address(offset)?;

        write_generic(&mut self.core, address, value)
    }

    pub(super) fn field_address(&self, offset: u32) -> JavaResult<u32> {
        let raw = self.read_raw()?;

        Ok(raw.ptr_fields + offset + 4)
    }

    pub(super) fn instantiate(core: &mut ArmCore, class: &JavaClass, field_size: JavaWord) -> JavaResult<Self> {
        let ptr_raw = Allocator::alloc(core, size_of::<RawJavaClassInstance>() as _)?;
        let ptr_fields = Allocator::alloc(core, (field_size + 4) as _)?;

        let zero = iter::repeat(0).take((field_size + 4) as _).collect::<Vec<_>>();
        core.write_bytes(ptr_fields, &zero)?;

        let vtable_index = JavaContextData::get_vtable_index(core, class)?;

        write_generic(
            core,
            ptr_raw,
            RawJavaClassInstance {
                ptr_fields,
                ptr_class: class.ptr_raw,
            },
        )?;
        write_generic(core, ptr_fields, (vtable_index * 4) << 5)?;

        tracing::trace!("Instantiate {}, vtable_index {:#x}", class.name()?, vtable_index);

        Ok(Self::from_raw(ptr_raw, core))
    }

    fn read_raw(&self) -> JavaResult<RawJavaClassInstance> {
        let instance: RawJavaClassInstance = read_generic(&self.core, self.ptr_raw as _)?;

        Ok(instance)
    }
}

impl ClassInstance for JavaClassInstance {
    fn destroy(self: Box<Self>) {
        (*self).destroy().unwrap()
    }

    fn class_name(&self) -> String {
        self.class().unwrap().name().unwrap()
    }

    fn get_field(&self, field: &dyn Field) -> JvmResult<JavaValue> {
        let field = field.as_any().downcast_ref::<JavaField>().unwrap();

        let result = self.read_field(field)?;

        Ok(JavaValue::from_raw(result, &field.descriptor(), &self.core))
    }

    fn put_field(&mut self, field: &dyn Field, value: JavaValue) -> JvmResult<()> {
        let field = field.as_any().downcast_ref::<JavaField>().unwrap();

        self.write_field(field, value.as_raw())
    }

    fn as_array_instance(&self) -> Option<&dyn ArrayClassInstance> {
        None
    }

    fn as_array_instance_mut(&mut self) -> Option<&mut dyn ArrayClassInstance> {
        None
    }
}
