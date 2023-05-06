use crate::wipi::java::{JavaBridge, JavaClassProto, JavaMethodProto, JavaObjectProxy, JavaResult};

// class java.lang.Runtime
pub struct Runtime {}

impl Runtime {
    pub fn as_proto() -> JavaClassProto {
        JavaClassProto {
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init),
                JavaMethodProto::new("getRuntime", "()Ljava/lang/Runtime;", Self::get_runtime),
                JavaMethodProto::new("totalMemory", "()J", Self::total_memory),
            ],
        }
    }

    fn init(_: &mut dyn JavaBridge) -> JavaResult<()> {
        log::debug!("Runtime::<init>");

        Ok(())
    }

    fn get_runtime(bridge: &mut dyn JavaBridge) -> JavaResult<JavaObjectProxy> {
        log::debug!("Runtime::get_runtime");

        let instance = bridge.instantiate("Ljava/lang/Runtime;")?;

        bridge.call_method(&instance, "<init>", "()V", &[])?;

        Ok(instance)
    }

    fn total_memory(_: &mut dyn JavaBridge) -> JavaResult<u32> {
        log::debug!("Runtime::total_memory");

        Ok(0x100000) // TODO: hardcoded
    }
}