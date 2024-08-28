use std::fmt;

use bytes::Bytes;
use serde::{de::Visitor, forward_to_deserialize_any};
use serde_json::{de::StrRead, Deserializer as JsonDeserializer};

pub struct Deserializer<'de> {
    inner: JsonDeserializer<StrRead<'de>>,
    binary_payloads: Vec<Bytes>,
    is_root: bool,
}

pub struct MultiDataVisitor<V> {
    inner: V,
}

struct BinaryVisitor<'a, V> {
    binary_payloads: &'a Vec<Bytes>,
    inner: V,
}

impl<'de> Deserializer<'de> {
    pub fn new(data: &'de str, binary_payloads: Vec<Bytes>) -> Self {
        let inner = JsonDeserializer::from_str(data);
        Self {
            inner,
            binary_payloads,
            is_root: true,
        }
    }
}

impl<'de, 'a> serde::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = <&'a mut JsonDeserializer<StrRead<'a>> as serde::Deserializer<'a>>::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(visitor)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_bool(visitor)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_i8(visitor)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_i16(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_i32(visitor)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_i64(visitor)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_u8(visitor)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_u16(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_u32(visitor)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_u64(visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_f32(visitor)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_f64(visitor)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_char(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_str(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_string(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_bytes(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_byte_buf(BinaryVisitor {
            inner: visitor,
            binary_payloads: &self.binary_payloads,
        })
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_option(visitor)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_unit(visitor)
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_unit_struct(name, visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_newtype_struct(name, visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.is_root {
            self.inner
                .deserialize_seq(MultiDataVisitor { inner: visitor })
        } else {
            self.is_root = false;
            self.inner.deserialize_seq(visitor)
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.is_root {
            self.inner
                .deserialize_tuple(len, MultiDataVisitor { inner: visitor })
        } else {
            self.is_root = false;
            self.inner.deserialize_tuple(len, visitor)
        }
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.is_root {
            self.inner
                .deserialize_tuple_struct(name, len, MultiDataVisitor { inner: visitor })
        } else {
            self.is_root = false;
            self.inner.deserialize_tuple_struct(name, len, visitor)
        }
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.is_root = false;
        self.inner.deserialize_map(visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.is_root = false;
        self.inner.deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.is_root = false;
        self.inner.deserialize_enum(name, variants, visitor)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_identifier(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_ignored_any(visitor)
    }
}

impl<'de, 'a, V: Visitor<'de>> Visitor<'de> for BinaryVisitor<'a, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a binary payload")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        use serde::de::Error;
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Val {
            Placeholder(bool),
            Num(usize),
        }

        let (key, val) = map
            .next_entry::<&str, Val>()?
            .ok_or(A::Error::custom("expected a binary placeholder"))?;
        let (key1, val1) = map
            .next_entry::<&str, Val>()?
            .ok_or(A::Error::custom("expected a binary placeholder"))?;

        match (key, val, key1, val1) {
            ("_placeholder", Val::Placeholder(true), "num", Val::Num(idx))
            | ("num", Val::Num(idx), "_placeholder", Val::Placeholder(true)) => {
                let payload = self
                    .binary_payloads
                    .get(idx)
                    .ok_or_else(|| A::Error::custom(format!("binary payload {} not found", idx)))?;
                self.inner.visit_byte_buf(payload.to_vec())
            }
            _ => Err(A::Error::custom("expected a binary placeholder")),
        }
    }

    fn visit_borrowed_bytes<E: serde::de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_bytes(v)
    }

    fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        self.inner.visit_byte_buf(v)
    }

    fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.inner.visit_bytes(v)
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for MultiDataVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a sequence")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let _ = seq.next_element::<&str>()?; // We ignore the event value
        self.inner.visit_seq(seq)
    }
}

struct IsTupleDeserializer {
    is_tuple: bool,
}

impl<'a, 'de> serde::Deserializer<'de> for &'a mut IsTupleDeserializer {
    type Error = serde_json::Error;

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str
        string unit unit_struct seq  map
        struct enum identifier ignored_any bytes byte_buf option
    }

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        use serde::de::Error;
        Err(Self::Error::custom(""))
    }

    fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        use serde::de::Error;
        self.is_tuple = true;
        Err(Self::Error::custom(""))
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        use serde::de::Error;
        self.is_tuple = true;
        Err(Self::Error::custom(""))
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        use serde::de::Error;
        self.is_tuple = true;
        Err(Self::Error::custom(""))
    }
}

pub fn is_tuple<'a, T: serde::Deserialize<'a>>() -> bool {
    let mut deser = IsTupleDeserializer { is_tuple: false };
    T::deserialize(&mut deser).ok();
    deser.is_tuple
}
