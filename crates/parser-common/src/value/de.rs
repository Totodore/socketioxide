use std::fmt;

use bytes::Bytes;
use serde::{
    de::{self, Visitor},
    forward_to_deserialize_any,
};
use serde_json::Deserializer as JsonDeserializer;

pub fn from_str<'de, T: de::Deserialize<'de>>(
    data: &'de str,
    binary_payloads: &Vec<Bytes>,
    with_event: bool,
) -> Result<T, serde_json::Error> {
    let inner = &mut JsonDeserializer::from_str(data);
    let de = Deserializer {
        inner,
        binary_payloads,
        skip_first_element: with_event,
    };
    T::deserialize(de)
}

pub fn from_str_seed<'de, T: de::DeserializeSeed<'de>>(
    data: &'de str,
    binary_payloads: &Vec<Bytes>,
    seed: T,
    with_event: bool,
) -> Result<T::Value, serde_json::Error> {
    let inner = &mut JsonDeserializer::from_str(data);
    let de = Deserializer {
        inner,
        binary_payloads,
        skip_first_element: with_event,
    };
    seed.deserialize(de)
}

pub struct Deserializer<'a, D> {
    inner: D,
    binary_payloads: &'a Vec<Bytes>,
    skip_first_element: bool,
}

impl<'a, 'de, D: de::Deserializer<'de>> de::Deserializer<'de> for Deserializer<'a, D> {
    type Error = D::Error;

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
        self.inner.deserialize_map(BinaryVisitor {
            inner: visitor,
            binary_payloads: self.binary_payloads,
        })
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_map(BinaryVisitor {
            inner: visitor,
            binary_payloads: self.binary_payloads,
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
        self.inner.deserialize_seq(WrapperVisitor::new(
            SeqVisitor::new(visitor, self.skip_first_element),
            self.binary_payloads,
        ))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_tuple(
            len,
            WrapperVisitor::new(
                SeqVisitor::new(visitor, self.skip_first_element),
                self.binary_payloads,
            ),
        )
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_tuple_struct(
            name,
            len,
            WrapperVisitor::new(
                SeqVisitor::new(visitor, self.skip_first_element),
                self.binary_payloads,
            ),
        )
    }

    fn deserialize_map<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Self::Error> {
        self.skip_first_element = false;
        self.inner
            .deserialize_map(WrapperVisitor::new(visitor, self.binary_payloads))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.skip_first_element = false;
        let visitor = WrapperVisitor::new(visitor, self.binary_payloads);
        self.inner.deserialize_struct(name, fields, visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        mut self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.skip_first_element = false;
        let visitor = WrapperVisitor::new(visitor, self.binary_payloads);
        self.inner.deserialize_enum(name, variants, visitor)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_identifier(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_ignored_any(visitor)
    }
}

struct WrapperVisitor<'a, I> {
    inner: I,
    binary_payloads: &'a Vec<Bytes>,
}
impl<'a, I> WrapperVisitor<'a, I> {
    fn new(inner: I, binary_payloads: &'a Vec<Bytes>) -> Self {
        Self {
            inner,
            binary_payloads,
        }
    }
}

impl<'a, 'de, I: Visitor<'de>> Visitor<'de> for WrapperVisitor<'a, I> {
    type Value = I::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_bool(v)
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_i8(v)
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_i16(v)
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_i32(v)
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_i64(v)
    }

    fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_i128(v)
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_u8(v)
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_u16(v)
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_u32(v)
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_u64(v)
    }

    fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_u128(v)
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_f32(v)
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_f64(v)
    }

    fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_char(v)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_str(v)
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_borrowed_str(v)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_string(v)
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_bytes(v)
    }

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_borrowed_bytes(v)
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_byte_buf(v)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_none()
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        self.inner.visit_some(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.inner.visit_unit()
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        self.inner.visit_newtype_struct(deserializer)
    }

    fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        self.inner.visit_seq(WrapperVisitor {
            inner: seq,
            binary_payloads: self.binary_payloads,
        })
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        self.inner.visit_map(WrapperVisitor {
            inner: map,
            binary_payloads: self.binary_payloads,
        })
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: de::EnumAccess<'de>,
    {
        self.inner.visit_enum(WrapperVisitor {
            inner: data,
            binary_payloads: self.binary_payloads,
        })
    }
}

impl<'a, 'de, I: de::SeqAccess<'de>> de::SeqAccess<'de> for WrapperVisitor<'a, I> {
    type Error = I::Error;

    fn next_element_seed<T: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        self.inner.next_element_seed(WrapperSeed {
            seed,
            binary_payloads: self.binary_payloads,
        })
    }
}
impl<'a, 'de, I: de::MapAccess<'de>> de::MapAccess<'de> for WrapperVisitor<'a, I> {
    type Error = I::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        self.inner.next_key_seed(WrapperSeed {
            seed,
            binary_payloads: self.binary_payloads,
        })
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        self.inner.next_value_seed(WrapperSeed {
            seed,
            binary_payloads: self.binary_payloads,
        })
    }
}
impl<'a, 'de, I: de::EnumAccess<'de>> de::EnumAccess<'de> for WrapperVisitor<'a, I> {
    type Error = I::Error;
    type Variant = I::Variant;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        self.inner.variant_seed(WrapperSeed {
            seed,
            binary_payloads: self.binary_payloads,
        })
    }
}

struct WrapperSeed<'a, S> {
    seed: S,
    binary_payloads: &'a Vec<Bytes>,
}
impl<'a, 'de, S: de::DeserializeSeed<'de>> de::DeserializeSeed<'de> for WrapperSeed<'a, S> {
    type Value = S::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        self.seed.deserialize(Deserializer {
            skip_first_element: false,
            inner: deserializer,
            binary_payloads: self.binary_payloads,
        })
    }
}

struct BinaryVisitor<'a, V> {
    binary_payloads: &'a Vec<Bytes>,
    inner: V,
}

impl<'a, 'de, V: de::Visitor<'de>> Visitor<'de> for BinaryVisitor<'a, V> {
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
                self.inner.visit_byte_buf(payload.to_vec()) //TODO: payload is copied here
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

pub struct SeqVisitor<V> {
    inner: V,
    skip_first_element: bool,
}
impl<V> SeqVisitor<V> {
    pub fn new(inner: V, skip_first_element: bool) -> Self {
        Self {
            inner,
            skip_first_element,
        }
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for SeqVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a sequence")
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        if self.skip_first_element {
            let _ = seq.next_element::<&str>()?; // We ignore the event value
        }
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

pub fn is_tuple<'de, T: serde::Deserialize<'de>>() -> bool {
    let mut deser = IsTupleDeserializer { is_tuple: false };
    T::deserialize(&mut deser).ok();
    deser.is_tuple
}
