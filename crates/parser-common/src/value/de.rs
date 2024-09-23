use std::fmt;

use bytes::Bytes;
use serde::de::{self, DeserializeSeed, IgnoredAny, Visitor};
use socketioxide_core::parser::FirstElement;

pub fn from_str<'de, T: de::Deserialize<'de>>(
    data: &'de str,
    binary_payloads: &Vec<Bytes>,
    with_event: bool,
) -> Result<T, serde_json::Error> {
    let inner = &mut serde_json::Deserializer::from_str(data);
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
    let inner = &mut serde_json::Deserializer::from_str(data);
    let de = Deserializer {
        inner,
        binary_payloads,
        skip_first_element: with_event,
    };
    seed.deserialize(de)
}

pub fn read_event(data: &str) -> serde_json::Result<&str> {
    let mut de = serde_json::Deserializer::from_str(data);
    FirstElement::<&str>::default().deserialize(&mut de)
}

struct Deserializer<'a, D> {
    inner: D,
    binary_payloads: &'a Vec<Bytes>,
    skip_first_element: bool,
}

impl<'a, 'de, D: de::Deserializer<'de>> de::Deserializer<'de> for Deserializer<'a, D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(BinaryAnyVisitor {
            inner: visitor,
            binary_payloads: self.binary_payloads,
        })
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

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        self.inner.visit_bool(v)
    }

    fn visit_i8<E: de::Error>(self, v: i8) -> Result<Self::Value, E> {
        self.inner.visit_i8(v)
    }

    fn visit_i16<E: de::Error>(self, v: i16) -> Result<Self::Value, E> {
        self.inner.visit_i16(v)
    }

    fn visit_i32<E: de::Error>(self, v: i32) -> Result<Self::Value, E> {
        self.inner.visit_i32(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.inner.visit_i64(v)
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Self::Value, E> {
        self.inner.visit_i128(v)
    }

    fn visit_u8<E: de::Error>(self, v: u8) -> Result<Self::Value, E> {
        self.inner.visit_u8(v)
    }

    fn visit_u16<E: de::Error>(self, v: u16) -> Result<Self::Value, E> {
        self.inner.visit_u16(v)
    }

    fn visit_u32<E: de::Error>(self, v: u32) -> Result<Self::Value, E> {
        self.inner.visit_u32(v)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.inner.visit_u64(v)
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
        self.inner.visit_u128(v)
    }

    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Self::Value, E> {
        self.inner.visit_f32(v)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        self.inner.visit_f64(v)
    }

    fn visit_char<E: de::Error>(self, v: char) -> Result<Self::Value, E> {
        self.inner.visit_char(v)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.inner.visit_str(v)
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_str(v)
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.inner.visit_string(v)
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.inner.visit_bytes(v)
    }

    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_bytes(v)
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
        self.inner.visit_byte_buf(v)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        self.inner.visit_some(deserializer)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }

    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(deserializer)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_seq(WrapperVisitor::new(seq, self.binary_payloads))
    }

    fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_map(WrapperVisitor::new(map, self.binary_payloads))
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_enum(WrapperVisitor::new(data, self.binary_payloads))
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

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
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

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
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

/// This custom [`SeqVisitor`] implementation is used to skip the first element of the sequence
/// if the `skip_first_element` field is set to `true`.
///
/// This is useful when the first element of the sequence is an event value that we want to ignore.
struct SeqVisitor<V> {
    inner: V,
    skip_first_element: bool,
}
impl<V> SeqVisitor<V> {
    fn new(inner: V, skip_first_element: bool) -> Self {
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
            let _ = seq.next_element::<IgnoredAny>()?; // We ignore the event value
        }
        self.inner.visit_seq(seq)
    }
}

/// A [`de::Deserializer`] that only visit the str key it holds.
struct PeekKey<'de, E> {
    phantom: std::marker::PhantomData<E>,
    key: &'de str,
}
impl<'de, E: de::Error> PeekKey<'de, E> {
    fn new(key: &'de str) -> Self {
        Self {
            phantom: std::marker::PhantomData,
            key,
        }
    }
}
impl<'de, E: de::Error> de::Deserializer<'de> for PeekKey<'de, E> {
    type Error = E;
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
        string unit unit_struct seq str map tuple tuple_struct newtype_struct
        struct enum identifier ignored_any bytes byte_buf option
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.key)
    }
}
/// A [`de::MapAccess`] implementation that can reinsert the key that
/// was peeked into the map we were reading from.
struct PeekKeyMap<'a, 'de, I> {
    binary_payloads: &'a Vec<Bytes>,
    inner: I,
    key: Option<&'de str>,
}
impl<'a, 'de, A: de::MapAccess<'de>> de::MapAccess<'de> for PeekKeyMap<'a, 'de, A> {
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.key.take() {
            Some(key) => seed.deserialize(PeekKey::new(key)).map(Some),
            None => self.inner.next_key_seed(seed),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        self.inner.next_value_seed(WrapperSeed {
            seed,
            binary_payloads: self.binary_payloads,
        })
    }
}
/// The [`BinaryAnyVisitor`] will check if in there is a `_placeholder` key in the map and if so,
/// it will replace the value with the binary payload at the index specified in the value.
struct BinaryAnyVisitor<'a, V> {
    binary_payloads: &'a Vec<Bytes>,
    inner: V,
}

impl<'a, 'de, V: de::Visitor<'de>> Visitor<'de> for BinaryAnyVisitor<'a, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a binary payload")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        use serde::de::Error;

        // We only check the first _placeholder key. Thanks to this if it is not present we can just
        // forward the map to the inner visitor with a custom wrapper as if the key was peeked.

        let key = map.next_key::<&str>()?;
        match key {
            Some("_placeholder") if !self.binary_payloads.is_empty() => {
                match map.next_value::<bool>()? {
                    true if map.next_key::<&str>()? == Some("num") => {
                        let idx = map.next_value::<usize>()?;
                        let payload = self.binary_payloads.get(idx).ok_or_else(|| {
                            A::Error::custom(format!("binary payload {} not found", idx))
                        })?;
                        self.inner.visit_byte_buf(payload.to_vec()) //TODO: payload is copied here
                    }
                    _ => Err(A::Error::custom("expected a binary placeholder")),
                }
            }
            _ => self.inner.visit_map(PeekKeyMap {
                inner: map,
                binary_payloads: &self.binary_payloads,
                key,
            }),
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

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        self.inner.visit_bool(v)
    }

    fn visit_i8<E: de::Error>(self, v: i8) -> Result<Self::Value, E> {
        self.inner.visit_i8(v)
    }

    fn visit_i16<E: de::Error>(self, v: i16) -> Result<Self::Value, E> {
        self.inner.visit_i16(v)
    }

    fn visit_i32<E: de::Error>(self, v: i32) -> Result<Self::Value, E> {
        self.inner.visit_i32(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.inner.visit_i64(v)
    }

    fn visit_i128<E: de::Error>(self, v: i128) -> Result<Self::Value, E> {
        self.inner.visit_i128(v)
    }

    fn visit_u8<E: de::Error>(self, v: u8) -> Result<Self::Value, E> {
        self.inner.visit_u8(v)
    }

    fn visit_u16<E: de::Error>(self, v: u16) -> Result<Self::Value, E> {
        self.inner.visit_u16(v)
    }

    fn visit_u32<E: de::Error>(self, v: u32) -> Result<Self::Value, E> {
        self.inner.visit_u32(v)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.inner.visit_u64(v)
    }

    fn visit_u128<E: de::Error>(self, v: u128) -> Result<Self::Value, E> {
        self.inner.visit_u128(v)
    }

    fn visit_f32<E: de::Error>(self, v: f32) -> Result<Self::Value, E> {
        self.inner.visit_f32(v)
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        self.inner.visit_f64(v)
    }

    fn visit_char<E: de::Error>(self, v: char) -> Result<Self::Value, E> {
        self.inner.visit_char(v)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.inner.visit_str(v)
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        self.inner.visit_borrowed_str(v)
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.inner.visit_string(v)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        self.inner.visit_some(deserializer)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }

    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(deserializer)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_seq(WrapperVisitor::new(seq, self.binary_payloads))
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_enum(WrapperVisitor::new(data, self.binary_payloads))
    }
}
