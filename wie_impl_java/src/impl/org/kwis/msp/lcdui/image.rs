use alloc::{boxed::Box, vec, vec::Vec};
use core::{
    cell::Ref,
    ops::{Deref, DerefMut},
};

use bytemuck::cast_vec;

use wie_backend::canvas::{create_canvas, decode_image, Canvas, Image as BackendImage, PixelFormat};

use crate::{
    base::{JavaClassProto, JavaContext, JavaFieldProto, JavaMethodFlag, JavaMethodProto, JavaResult},
    proxy::{Array, JvmClassInstanceProxy},
    r#impl::{java::lang::String, org::kwis::msp::lcdui::Graphics},
    JavaFieldAccessFlag,
};

// class org.kwis.msp.lcdui.Image
pub struct Image {}

impl Image {
    pub fn as_proto() -> JavaClassProto {
        JavaClassProto {
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, JavaMethodFlag::NONE),
                JavaMethodProto::new(
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image,
                    JavaMethodFlag::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image_from_file,
                    JavaMethodFlag::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "([BII)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image_from_bytes,
                    JavaMethodFlag::STATIC,
                ),
                JavaMethodProto::new("getGraphics", "()Lorg/kwis/msp/lcdui/Graphics;", Self::get_graphics, JavaMethodFlag::NONE),
                JavaMethodProto::new("getWidth", "()I", Self::get_width, JavaMethodFlag::NONE),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, JavaMethodFlag::NONE),
            ],
            fields: vec![
                JavaFieldProto::new("w", "I", JavaFieldAccessFlag::NONE),
                JavaFieldProto::new("h", "I", JavaFieldAccessFlag::NONE),
                JavaFieldProto::new("imgData", "[B", JavaFieldAccessFlag::NONE),
                JavaFieldProto::new("bpl", "I", JavaFieldAccessFlag::NONE),
            ],
        }
    }

    async fn init(_: &mut dyn JavaContext, this: JvmClassInstanceProxy<Image>) -> JavaResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Image::<init>({:?})", &this);

        Ok(())
    }

    async fn create_image(context: &mut dyn JavaContext, width: i32, height: i32) -> JavaResult<JvmClassInstanceProxy<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({}, {})", width, height);

        let bytes_per_pixel = 4;

        Self::create_image_instance(
            context,
            width as _,
            height as _,
            &vec![0; (width * height * bytes_per_pixel) as usize],
            bytes_per_pixel as _,
        )
        .await
    }

    #[allow(clippy::await_holding_refcell_ref)] // We manually drop Ref
    async fn create_image_from_file(context: &mut dyn JavaContext, name: JvmClassInstanceProxy<String>) -> JavaResult<JvmClassInstanceProxy<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({:?})", &name);

        let name = String::to_rust_string(context, &name)?;
        let normalized_name = if let Some(x) = name.strip_prefix('/') { x } else { &name };

        let id = context.system().resource().id(normalized_name).unwrap();
        let system_clone = context.system().clone();
        let image_data = Ref::map(system_clone.resource(), |x| x.data(id));

        let image = decode_image(&image_data)?;
        drop(image_data);

        Self::create_image_instance(context, image.width(), image.height(), image.raw(), image.bytes_per_pixel()).await
    }

    async fn create_image_from_bytes(
        context: &mut dyn JavaContext,
        data: JvmClassInstanceProxy<Array<i8>>,
        offset: i32,
        length: i32,
    ) -> JavaResult<JvmClassInstanceProxy<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({:?}, {}, {})", &data, offset, length);

        let image_data: Vec<i8> = context.jvm().load_array(&data, offset as _, length as _)?;
        let image = decode_image(&cast_vec(image_data))?;

        Self::create_image_instance(context, image.width(), image.height(), image.raw(), image.bytes_per_pixel()).await
    }

    async fn get_graphics(context: &mut dyn JavaContext, this: JvmClassInstanceProxy<Self>) -> JavaResult<JvmClassInstanceProxy<Graphics>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getGraphics({:?})", &this);

        let width: i32 = context.jvm().get_field(&this, "w", "I")?;
        let height: i32 = context.jvm().get_field(&this, "h", "I")?;

        let instance = context
            .jvm()
            .new_class(
                "org/kwis/msp/lcdui/Graphics",
                "(Lorg/kwis/msp/lcdui/Image;IIII)V",
                (this.clone(), 0, 0, width, height),
            )
            .await?;

        Ok(instance.into())
    }

    async fn get_width(context: &mut dyn JavaContext, this: JvmClassInstanceProxy<Self>) -> JavaResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getWidth({:?})", &this);

        context.jvm().get_field(&this, "w", "I")
    }

    async fn get_height(context: &mut dyn JavaContext, this: JvmClassInstanceProxy<Self>) -> JavaResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getHeight({:?})", &this);

        context.jvm().get_field(&this, "h", "I")
    }

    pub fn buf(context: &mut dyn JavaContext, this: &JvmClassInstanceProxy<Self>) -> JavaResult<Vec<u8>> {
        let java_img_data = context.jvm().get_field(this, "imgData", "[B")?;
        let img_data_len = context.jvm().array_length(&java_img_data)?;

        let img_data: Vec<i8> = context.jvm().load_array(&java_img_data, 0, img_data_len)?;

        Ok(cast_vec(img_data))
    }

    pub fn image(context: &mut dyn JavaContext, this: &JvmClassInstanceProxy<Self>) -> JavaResult<Box<dyn BackendImage>> {
        Ok(Self::create_canvas(context, this)?.image())
    }

    pub fn canvas<'a>(context: &'a mut dyn JavaContext, this: &'a JvmClassInstanceProxy<Self>) -> JavaResult<ImageCanvas<'a>> {
        let canvas = Self::create_canvas(context, this)?;

        Ok(ImageCanvas {
            image: this,
            context,
            canvas,
        })
    }

    fn create_canvas(context: &mut dyn JavaContext, this: &JvmClassInstanceProxy<Self>) -> JavaResult<Box<dyn Canvas>> {
        let buf = Self::buf(context, this)?;

        let width: i32 = context.jvm().get_field(this, "w", "I")?;
        let height: i32 = context.jvm().get_field(this, "h", "I")?;
        let bpl: i32 = context.jvm().get_field(this, "bpl", "I")?;

        let bytes_per_pixel = bpl / width;

        let pixel_format = match bytes_per_pixel {
            2 => PixelFormat::Rgb565,
            4 => PixelFormat::Argb,
            _ => panic!("Unsupported pixel format: {}", bytes_per_pixel),
        };

        create_canvas(width as _, height as _, pixel_format, &buf)
    }

    async fn create_image_instance(
        context: &mut dyn JavaContext,
        width: u32,
        height: u32,
        data: &[u8],
        bytes_per_pixel: u32,
    ) -> JavaResult<JvmClassInstanceProxy<Image>> {
        let instance = context.jvm().new_class("org/kwis/msp/lcdui/Image", "()V", []).await?;

        let data: Vec<i8> = cast_vec(data.to_vec());

        let data_array = context.jvm().instantiate_array("B", data.len() as _).await?;
        context.jvm().store_array(&data_array, 0, data)?;

        context.jvm().put_field(&instance, "w", "I", width as i32)?;
        context.jvm().put_field(&instance, "h", "I", height as i32)?;
        context.jvm().put_field(&instance, "imgData", "[B", data_array)?;
        context.jvm().put_field(&instance, "bpl", "I", (width * bytes_per_pixel) as i32)?;

        Ok(instance.into())
    }
}

pub struct ImageCanvas<'a> {
    image: &'a JvmClassInstanceProxy<Image>,
    context: &'a mut dyn JavaContext,
    canvas: Box<dyn Canvas>,
}

impl Drop for ImageCanvas<'_> {
    fn drop(&mut self) {
        let data = self.context.jvm().get_field(self.image, "imgData", "[B").unwrap();

        let values: Vec<i8> = cast_vec(self.canvas.raw().to_vec());

        self.context.jvm().store_array(&data, 0, values).unwrap();
    }
}

impl Deref for ImageCanvas<'_> {
    type Target = Box<dyn Canvas>;

    fn deref(&self) -> &Self::Target {
        &self.canvas
    }
}

impl DerefMut for ImageCanvas<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.canvas
    }
}
