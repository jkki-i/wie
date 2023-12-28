use core::{
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
};

use jvm::{ClassInstanceRef, JavaValue};

use crate::{base::JavaContext, method::TypeConverter};

pub struct JvmClassInstanceProxy<T> {
    pub class_instance: Option<ClassInstanceRef>,
    _phantom: PhantomData<T>,
}

impl<T> JvmClassInstanceProxy<T> {
    pub fn new(class_instance: Option<ClassInstanceRef>) -> Self {
        Self {
            class_instance,
            _phantom: PhantomData,
        }
    }
}

impl<T> TypeConverter<JvmClassInstanceProxy<T>> for JvmClassInstanceProxy<T> {
    fn to_rust(_: &mut dyn JavaContext, raw: JavaValue) -> JvmClassInstanceProxy<T> {
        JvmClassInstanceProxy::new(raw.as_object())
    }

    fn from_rust(_: &mut dyn JavaContext, value: JvmClassInstanceProxy<T>) -> JavaValue {
        JavaValue::Object(value.class_instance)
    }
}

impl<T> Debug for JvmClassInstanceProxy<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(x) = &self.class_instance {
            write!(f, "{:?}", x.borrow())
        } else {
            write!(f, "null")
        }
    }
}

pub struct JvmArrayClassInstanceProxy<T> {
    pub class_instance: Option<ClassInstanceRef>,
    _phantom: PhantomData<T>,
}

impl<T> JvmArrayClassInstanceProxy<T> {
    pub fn new(class_instance: Option<ClassInstanceRef>) -> Self {
        Self {
            class_instance,
            _phantom: PhantomData,
        }
    }
}

impl<T> TypeConverter<JvmArrayClassInstanceProxy<T>> for JvmArrayClassInstanceProxy<T> {
    fn to_rust(_: &mut dyn JavaContext, raw: JavaValue) -> JvmArrayClassInstanceProxy<T> {
        JvmArrayClassInstanceProxy::new(raw.as_object())
    }

    fn from_rust(_: &mut dyn JavaContext, value: JvmArrayClassInstanceProxy<T>) -> JavaValue {
        JavaValue::Object(value.class_instance)
    }
}

impl<T> Debug for JvmArrayClassInstanceProxy<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(x) = &self.class_instance {
            write!(f, "{:?}", x.borrow())
        } else {
            write!(f, "null")
        }
    }
}
