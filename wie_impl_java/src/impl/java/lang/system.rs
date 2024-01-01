use alloc::{vec, vec::Vec};

use jvm::JavaValue;

use crate::{
    base::{JavaClassProto, JavaContext, JavaFieldProto, JavaMethodFlag, JavaMethodProto, JavaResult},
    proxy::{Array, JvmClassInstanceProxy},
    JavaFieldAccessFlag,
};

// class java.lang.System
pub struct System {}

impl System {
    pub fn as_proto() -> JavaClassProto {
        JavaClassProto {
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, JavaMethodFlag::STATIC),
                JavaMethodProto::new("currentTimeMillis", "()J", Self::current_time_millis, JavaMethodFlag::NATIVE),
                JavaMethodProto::new("gc", "()V", Self::gc, JavaMethodFlag::STATIC),
                JavaMethodProto::new(
                    "arraycopy",
                    "(Ljava/lang/Object;ILjava/lang/Object;II)V",
                    Self::arraycopy,
                    JavaMethodFlag::NATIVE,
                ),
            ],
            fields: vec![JavaFieldProto::new("out", "Ljava/io/PrintStream;", JavaFieldAccessFlag::STATIC)],
        }
    }

    async fn cl_init(context: &mut dyn JavaContext) -> JavaResult<()> {
        tracing::debug!("java.lang.System::<clinit>()");

        let out = context.jvm().new_class("java/io/PrintStream", "()V", []).await?;
        // TODO call constructor with dummy output stream?

        context
            .jvm()
            .put_static_field("java/lang/System", "out", "Ljava/io/PrintStream;", out)
            .await?;

        Ok(())
    }

    async fn current_time_millis(context: &mut dyn JavaContext) -> JavaResult<i64> {
        tracing::debug!("java.lang.System::currentTimeMillis()");

        Ok(context.system().time().now().raw() as _)
    }

    async fn gc(_: &mut dyn JavaContext) -> JavaResult<i32> {
        tracing::warn!("stub java.lang.System::gc()");

        Ok(0)
    }

    async fn arraycopy(
        context: &mut dyn JavaContext,
        src: JvmClassInstanceProxy<Array<()>>, // Any Array
        src_pos: i32,
        dest: JvmClassInstanceProxy<Array<()>>,
        dest_pos: i32,
        length: i32,
    ) -> JavaResult<()> {
        tracing::debug!(
            "java.lang.System::arraycopy({:?}, {}, {:?}, {}, {})",
            &src,
            src_pos,
            &dest,
            dest_pos,
            length
        );

        // TODO i think we can make it faster
        let src: Vec<JavaValue> = context.jvm().load_array(&src, src_pos as _, length as _)?;
        context.jvm().store_array(&dest, dest_pos as _, src)?;

        Ok(())
    }
}
