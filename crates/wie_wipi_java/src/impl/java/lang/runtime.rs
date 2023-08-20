use alloc::vec;

use crate::{
    base::{JavaClassProto, JavaContext, JavaMethodAccessFlag, JavaMethodProto, JavaResult},
    proxy::JavaObjectProxy,
};

// class java.lang.Runtime
pub struct Runtime {}

impl Runtime {
    pub fn as_proto() -> JavaClassProto {
        JavaClassProto {
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, JavaMethodAccessFlag::NONE),
                JavaMethodProto::new("getRuntime", "()Ljava/lang/Runtime;", Self::get_runtime, JavaMethodAccessFlag::NONE),
                JavaMethodProto::new("totalMemory", "()J", Self::total_memory, JavaMethodAccessFlag::NONE),
            ],
            fields: vec![],
        }
    }

    async fn init(_: &mut dyn JavaContext, this: JavaObjectProxy<Runtime>) -> JavaResult<()> {
        log::warn!("stub java.lang.Runtime::<init>({:#x})", this.ptr_instance);

        Ok(())
    }

    async fn get_runtime(context: &mut dyn JavaContext) -> JavaResult<JavaObjectProxy<Runtime>> {
        log::trace!("java.lang.Runtime::get_runtime");

        let instance = context.instantiate("Ljava/lang/Runtime;")?.cast();
        context.call_method(&instance.cast(), "<init>", "()V", &[]).await?;

        Ok(instance)
    }

    async fn total_memory(_: &mut dyn JavaContext) -> JavaResult<u32> {
        log::warn!("stub java.lang.Runtime::total_memory");

        Ok(0x100000) // TODO: hardcoded
    }
}
