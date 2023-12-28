use alloc::vec;

use jvm::JavaValue;

use crate::{
    base::{JavaClassProto, JavaContext, JavaFieldProto, JavaMethodFlag, JavaMethodProto, JavaResult},
    proxy::{JavaObjectProxy, JvmClassInstanceProxy},
};

// class org.kwis.msp.lcdui.Font
pub struct Font {}

impl Font {
    pub fn as_proto() -> JavaClassProto {
        JavaClassProto {
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, JavaMethodFlag::NONE),
                JavaMethodProto::new("<init>", "()V", Self::init, JavaMethodFlag::NONE),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, JavaMethodFlag::NONE),
                JavaMethodProto::new(
                    "getDefaultFont",
                    "()Lorg/kwis/msp/lcdui/Font;",
                    Self::get_default_font,
                    JavaMethodFlag::STATIC,
                ),
                JavaMethodProto::new("getFont", "(III)Lorg/kwis/msp/lcdui/Font;", Self::get_font, JavaMethodFlag::STATIC),
            ],
            fields: vec![
                JavaFieldProto::new("FACE_SYSTEM", "I", crate::JavaFieldAccessFlag::STATIC),
                JavaFieldProto::new("STYLE_PLAIN", "I", crate::JavaFieldAccessFlag::STATIC),
                JavaFieldProto::new("SIZE_SMALL", "I", crate::JavaFieldAccessFlag::STATIC),
            ],
        }
    }

    async fn cl_init(context: &mut dyn JavaContext, this: JvmClassInstanceProxy<Self>) -> JavaResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Font::<clinit>({:#x})", context.instance_raw(&this.class_instance));

        context
            .jvm()
            .put_static_field("org/kwis/msp/lcdui/Font", "FACE_SYSTEM", "I", JavaValue::Int(0))
            .await?;
        context
            .jvm()
            .put_static_field("org/kwis/msp/lcdui/Font", "STYLE_PLAIN", "I", JavaValue::Int(0))
            .await?;
        context
            .jvm()
            .put_static_field("org/kwis/msp/lcdui/Font", "SIZE_SMALL", "I", JavaValue::Int(8))
            .await?;

        Ok(())
    }

    async fn init(_: &mut dyn JavaContext, this: JavaObjectProxy<Font>) -> JavaResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Font::<init>({:#x})", this.ptr_instance);

        Ok(())
    }

    async fn get_height(_: &mut dyn JavaContext) -> JavaResult<i32> {
        tracing::warn!("stub org.kwis.msp.lcdui.Font::getHeight");

        Ok(12) // TODO: hardcoded
    }

    async fn get_default_font(context: &mut dyn JavaContext) -> JavaResult<JvmClassInstanceProxy<Self>> {
        tracing::warn!("stub org.kwis.msp.lcdui.Font::getDefaultFont");

        let instance = context.jvm().instantiate_class("org/kwis/msp/lcdui/Font").await?;
        context
            .jvm()
            .invoke_method(&instance, "org/kwis/msp/lcdui/Font", "<init>", "()V", &[])
            .await?;

        Ok(JvmClassInstanceProxy::new(instance))
    }

    async fn get_font(context: &mut dyn JavaContext, face: i32, style: i32, size: i32) -> JavaResult<JvmClassInstanceProxy<Font>> {
        tracing::warn!("stub org.kwis.msp.lcdui.Font::getFont({:#x}, {:#x}, {:#x})", face, style, size);

        let instance = context.jvm().instantiate_class("org/kwis/msp/lcdui/Font").await?;
        context
            .jvm()
            .invoke_method(&instance, "org/kwis/msp/lcdui/Font", "<init>", "()V", &[])
            .await?;

        Ok(JvmClassInstanceProxy::new(instance))
    }
}
