use std::{borrow::Cow, collections::HashMap};

use bytes::Bytes;
use serde_json::{Number, Value};

use super::PayloadValue;

macro_rules! impl_from_integer {
    ($ty:ty) => {
        impl From<$ty> for PayloadValue {
            fn from(value: $ty) -> Self {
                PayloadValue::Number(value.into())
            }
        }
    };
}

macro_rules! impl_from_binary {
    ($intty:ty) => {
        impl From<($intty, Bytes)> for PayloadValue {
            fn from(value: ($intty, Bytes)) -> Self {
                PayloadValue::Binary(value.0 as usize, value.1)
            }
        }
    };
}

impl From<()> for PayloadValue {
    fn from(_value: ()) -> Self {
        PayloadValue::Null
    }
}

impl From<bool> for PayloadValue {
    fn from(value: bool) -> Self {
        PayloadValue::Bool(value)
    }
}

impl_from_integer!(u8);
impl_from_integer!(i8);
impl_from_integer!(u16);
impl_from_integer!(i16);
impl_from_integer!(u32);
impl_from_integer!(i32);
impl_from_integer!(u64);
impl_from_integer!(i64);
impl_from_integer!(usize);
impl_from_integer!(isize);

impl From<Number> for PayloadValue {
    fn from(value: Number) -> Self {
        PayloadValue::Number(value)
    }
}

impl From<String> for PayloadValue {
    fn from(value: String) -> Self {
        PayloadValue::String(value)
    }
}

impl From<&str> for PayloadValue {
    fn from(value: &str) -> Self {
        PayloadValue::String(value.to_string())
    }
}

impl<'a> From<Cow<'a, str>> for PayloadValue {
    fn from(value: Cow<'a, str>) -> Self {
        match value {
            Cow::Owned(s) => PayloadValue::String(s),
            Cow::Borrowed(s) => PayloadValue::String(s.to_string()),
        }
    }
}

impl_from_binary!(u8);
impl_from_binary!(u16);
impl_from_binary!(u32);
impl_from_binary!(u64);
impl_from_binary!(usize);

impl<V: Into<PayloadValue>> From<HashMap<String, V>> for PayloadValue {
    fn from(value: HashMap<String, V>) -> Self {
        PayloadValue::Object(value.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

impl<T: Into<PayloadValue>> From<Vec<T>> for PayloadValue {
    fn from(value: Vec<T>) -> Self {
        PayloadValue::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<PayloadValue>, const N: usize> From<[T; N]> for PayloadValue {
    fn from(value: [T; N]) -> Self {
        PayloadValue::Array(value.into_iter().map(Into::into).collect())
    }
}

impl From<Value> for PayloadValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => PayloadValue::Null,
            Value::Bool(b) => PayloadValue::Bool(b),
            Value::Number(n) => PayloadValue::Number(n),
            Value::String(s) => PayloadValue::String(s),
            Value::Array(a) => PayloadValue::Array(a.into_iter().map(Into::into).collect()),
            Value::Object(o) => match (o.get("_placeholder"), o.get("num")) {
                (Some(Value::Bool(true)), Some(Value::Number(num)))
                    if num
                        .as_u64()
                        .map(|n| n <= usize::MAX as u64)
                        .unwrap_or(false) =>
                {
                    PayloadValue::Binary(num.as_u64().unwrap() as usize, Bytes::new())
                }
                _ => PayloadValue::Object(o.into_iter().map(|(k, v)| (k, v.into())).collect()),
            },
        }
    }
}

impl From<PayloadValue> for Value {
    fn from(value: PayloadValue) -> Self {
        match value {
            PayloadValue::Null => Value::Null,
            PayloadValue::Bool(b) => Value::Bool(b),
            PayloadValue::Number(n) => Value::Number(n),
            PayloadValue::String(s) => Value::String(s),
            PayloadValue::Binary(num, _) => Value::Object(
                [
                    ("_placeholder".to_string(), Value::Bool(true)),
                    ("num".to_string(), Value::Number(Number::from(num))),
                ]
                .into_iter()
                .collect(),
            ),
            PayloadValue::Array(a) => Value::Array(a.into_iter().map(Into::into).collect()),
            PayloadValue::Object(o) => {
                Value::Object(o.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
        }
    }
}

impl<V: Into<PayloadValue>> FromIterator<(String, V)> for PayloadValue {
    fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
        PayloadValue::Object(iter.into_iter().map(|(k, v)| (k, v.into())).collect())
    }
}

impl<T: Into<PayloadValue>> FromIterator<T> for PayloadValue {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        PayloadValue::Array(iter.into_iter().map(Into::into).collect())
    }
}
