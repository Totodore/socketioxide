use std::{borrow::Cow, marker::PhantomData};

use bytes::Bytes;
use serde::{
    de::{
        self, DeserializeSeed, EnumAccess, Error, IntoDeserializer, MapAccess, SeqAccess,
        Unexpected, VariantAccess,
    },
    forward_to_deserialize_any,
};
use serde_json::{Map, Value};

// For Deserializer impl for Value Deserializer wrapper
macro_rules! forward_deser {
    ($method:ident) => {
        #[inline]
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            self.value.$method(visitor)
        }
    };
}

// For Deserializer impl for MapKeyDeserializer
macro_rules! impl_deser_numeric_key {
    ($method:ident) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            // This is less efficient than it could be, but `Deserializer::from_str()` (used
            // internally by `serde_json`) wants to hold a reference to `self.key` longer than is
            // permitted.  The methods on `Deserializer` for deserializing into a `Number` without
            // that constraint are not exposed publicly.
            let reader = VecRead::from(self.key);
            let mut deser = serde_json::Deserializer::from_reader(reader);
            let number = deser.$method(visitor)?;
            let _ = deser
                .end()
                .map_err(|_| serde_json::Error::custom("expected numeric map key"))?;
            Ok(number)
        }
    };
}

struct Deserializer<'a, T> {
    value: serde_json::Value,
    binary_payloads: &'a [Bytes],
    _phantom: PhantomData<T>,
}

impl<'a, 'de, T: serde::Deserialize<'de>> serde::de::Deserializer<'de> for Deserializer<'a, T> {
    type Error = serde_json::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Array(a) => visit_value_array::<'a, 'de, V, T>(a, self.binary_payloads, visitor),
            Value::Object(o) => {
                visit_value_object::<'a, 'de, V, T>(o, self.binary_payloads, visitor)
            }
            other => other.deserialize_any(visitor),
        }
    }

    forward_deser!(deserialize_unit);
    forward_deser!(deserialize_bool);
    forward_deser!(deserialize_u8);
    forward_deser!(deserialize_i8);
    forward_deser!(deserialize_u16);
    forward_deser!(deserialize_i16);
    forward_deser!(deserialize_u32);
    forward_deser!(deserialize_i32);
    forward_deser!(deserialize_u64);
    forward_deser!(deserialize_i64);
    forward_deser!(deserialize_u128);
    forward_deser!(deserialize_i128);
    forward_deser!(deserialize_f32);
    forward_deser!(deserialize_f64);
    forward_deser!(deserialize_char);
    forward_deser!(deserialize_str);
    forward_deser!(deserialize_string);
    forward_deser!(deserialize_identifier);

    #[inline]
    fn deserialize_unit_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.value.deserialize_unit_struct(name, visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::String(s) => visitor.visit_bytes(s.as_bytes()),
            Value::Object(o) => visit_value_object_for_bytes(o, self.binary_payloads, visitor),
            Value::Array(a) => visit_value_array_for_bytes(a, visitor),
            _ => Err(serde_json::Error::invalid_type(
                unexpected_value(&self.value),
                &"byte array or binary payload",
            )),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    #[inline]
    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_none(),
            Value::Array(a) => visit_value_array::<'a, 'de, V, T>(a, self.binary_payloads, visitor),
            Value::Object(o) => {
                visit_value_object::<'a, 'de, V, T>(o, self.binary_payloads, visitor)
            }
            other => other.deserialize_option(visitor),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Array(a) => visit_value_array::<'a, 'de, V, T>(a, self.binary_payloads, visitor),
            Value::Object(o) => {
                visit_value_object::<'a, 'de, V, T>(o, self.binary_payloads, visitor)
            }
            _ => Err(serde_json::Error::invalid_type(
                unexpected_value(&self.value),
                &visitor,
            )),
        }
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Object(o) => {
                visit_value_object::<'a, 'de, V, T>(o, self.binary_payloads, visitor)
            }
            _ => Err(serde_json::Error::invalid_type(
                unexpected_value(&self.value),
                &visitor,
            )),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Array(a) => visit_value_array::<'a, 'de, V, T>(a, self.binary_payloads, visitor),
            _ => Err(serde_json::Error::invalid_type(
                unexpected_value(&self.value),
                &visitor,
            )),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Object(o) => {
                // From serde_json: enums are encoded as a map with _only_ a single key-value pair
                if o.len() == 1 {
                    let (variant, value) = o.into_iter().next().unwrap();
                    visitor.visit_enum(EnumDeserializer {
                        variant,
                        value: Some(value),
                        binary_payloads: self.binary_payloads,
                        _phantom: PhantomData::<T>,
                    })
                } else {
                    Err(serde_json::Error::invalid_value(
                        Unexpected::Map,
                        &"a map with a single key-value pair",
                    ))
                }
            }
            Value::String(s) => visitor.visit_enum(EnumDeserializer {
                variant: s,
                value: None,
                binary_payloads: self.binary_payloads,
                _phantom: PhantomData::<T>,
            }),
            _ => Err(serde_json::Error::invalid_type(
                unexpected_value(&self.value),
                &"map or string",
            )),
        }
    }

    #[inline]
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        drop(self);
        visitor.visit_unit()
    }
}

fn visit_value_object_for_bytes<'a: 'a, 'de, V>(
    o: Map<String, Value>,
    binary_payloads: &'a [Bytes],
    visitor: V,
) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
{
    if !o.len() == 2
        || !o
            .get("_placeholder")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        Err(serde_json::Error::invalid_type(
            Unexpected::Map,
            &"binary payload placeholder object",
        ))
    } else if let Some(num) = o.get("num").and_then(|v| v.as_u64()) {
        if let Some(payload) = binary_payloads.get(num as usize) {
            visitor.visit_bytes(payload)
        } else {
            Err(serde_json::Error::invalid_value(
                Unexpected::Unsigned(num),
                &"a payload number in range",
            ))
        }
    } else {
        Err(serde_json::Error::invalid_value(
            Unexpected::Map,
            &"binary payload placeholder without valid num",
        ))
    }
}

fn visit_value_array_for_bytes<'de, V>(
    a: Vec<Value>,
    visitor: V,
) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
{
    let bytes = a
        .into_iter()
        .map(|v| match v {
            Value::Number(n) => n
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| {
                    serde_json::Error::invalid_value(
                        Unexpected::Other("non-u8 number"),
                        &"number that fits in a u8",
                    )
                }),
            _ => Err(serde_json::Error::invalid_value(
                unexpected_value(&v),
                &"number that fits in a u8",
            )),
        })
        .collect::<Result<Vec<u8>, _>>()?;
    visitor.visit_bytes(&bytes)
}

fn visit_value_object<'a: 'a, 'de, V, T>(
    o: Map<String, Value>,
    binary_payloads: &'a [Bytes],
    visitor: V,
) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
    T: serde::Deserialize<'de>,
{
    let len = o.len();

    let mut deser = MapDeserializer {
        iter: o.into_iter(),
        binary_payloads,
        value: None,
        _phantom: PhantomData::<T>,
    };
    let map = visitor.visit_map(&mut deser)?;
    if deser.iter.len() == 0 {
        Ok(map)
    } else {
        Err(serde_json::Error::invalid_length(
            len,
            &"fewer elements in map",
        ))
    }
}

fn visit_value_array<'a: 'a, 'de, V, T>(
    a: Vec<Value>,
    binary_payloads: &'a [Bytes],
    visitor: V,
) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
    T: serde::Deserialize<'de>,
{
    let len = a.len();
    let mut deser = SeqDeserializer {
        iter: a.into_iter(),
        binary_payloads,
        _phantom: PhantomData::<T>,
    };
    let seq = visitor.visit_seq(&mut deser)?;
    if deser.iter.len() == 0 {
        Ok(seq)
    } else {
        Err(serde_json::Error::invalid_length(
            len,
            &"fewer elements in seq",
        ))
    }
}

struct MapDeserializer<'a, T> {
    iter: <Map<String, Value> as IntoIterator>::IntoIter,
    binary_payloads: &'a [Bytes],
    value: Option<Value>,
    _phantom: PhantomData<T>,
}

impl<'a, 'de, T: serde::Deserialize<'de>> MapAccess<'de> for MapDeserializer<'a, T> {
    type Error = serde_json::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                let key_deser = MapKeyDeserializer::from(key);
                seed.deserialize(key_deser).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(value) => {
                let payload = Deserializer {
                    value,
                    binary_payloads: self.binary_payloads,
                    _phantom: PhantomData::<T>,
                };
                seed.deserialize(payload)
            }
            None => Err(serde_json::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

struct SeqDeserializer<'a, T> {
    iter: <Vec<Value> as IntoIterator>::IntoIter,
    binary_payloads: &'a [Bytes],
    _phantom: PhantomData<T>,
}

impl<'a, 'de, T: serde::Deserialize<'de>> SeqAccess<'de> for SeqDeserializer<'a, T> {
    type Error = serde_json::Error;

    fn next_element_seed<S>(&mut self, seed: S) -> Result<Option<S::Value>, Self::Error>
    where
        S: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => {
                let payload = Deserializer {
                    value,
                    binary_payloads: self.binary_payloads,
                    _phantom: PhantomData::<T>,
                };
                seed.deserialize(payload).map(Some)
            }
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

struct EnumDeserializer<'a, T> {
    variant: String,
    value: Option<Value>,
    binary_payloads: &'a [Bytes],
    _phantom: PhantomData<T>,
}

impl<'a, 'de, T: serde::Deserialize<'de>> EnumAccess<'de> for EnumDeserializer<'a, T> {
    type Variant = VariantDeserializer<'a, T>;
    type Error = serde_json::Error;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = self.variant.into_deserializer();
        let visitor = VariantDeserializer {
            value: self.value,
            binary_payloads: self.binary_payloads,
            _phantom: PhantomData::<T>,
        };
        seed.deserialize(variant).map(|v| (v, visitor))
    }
}

struct VariantDeserializer<'a, T> {
    value: Option<Value>,
    binary_payloads: &'a [Bytes],
    _phantom: PhantomData<T>,
}

impl<'a, 'de, T: serde::Deserialize<'de>> VariantAccess<'de> for VariantDeserializer<'a, T> {
    type Error = serde_json::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            Some(value) => {
                let deser = Deserializer {
                    value,
                    binary_payloads: self.binary_payloads,
                    _phantom: PhantomData::<T>,
                };
                serde::Deserialize::deserialize(deser)
            }
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<S>(self, seed: S) -> Result<S::Value, Self::Error>
    where
        S: DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => {
                let deser = Deserializer {
                    value,
                    binary_payloads: self.binary_payloads,
                    _phantom: PhantomData::<T>,
                };
                seed.deserialize(deser)
            }
            None => Err(serde_json::Error::invalid_type(
                Unexpected::Unit,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(Value::Array(a)) => {
                if a.is_empty() {
                    visitor.visit_unit()
                } else {
                    visit_value_array::<'a, 'de, V, T>(a, self.binary_payloads, visitor)
                }
            }
            Some(other) => Err(serde_json::Error::invalid_type(
                unexpected_value(&other),
                &"tuple variant",
            )),
            None => Err(serde_json::Error::invalid_type(
                Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(Value::Object(o)) => {
                visit_value_object::<'a, 'de, V, T>(o, self.binary_payloads, visitor)
            }
            Some(other) => Err(serde_json::Error::invalid_type(
                unexpected_value(&other),
                &"struct variant",
            )),
            None => Err(serde_json::Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

/// Helper struct that implements `std::io::Read`, which allows us to use
/// `serde_json::Deserializer` to deserialize a string into a `serde_json::Number`.
struct VecRead {
    vec: Vec<u8>,
    pos: usize,
}

impl<'any> From<Cow<'any, str>> for VecRead {
    fn from(value: Cow<'any, str>) -> Self {
        Self {
            vec: value.to_string().into_bytes(),
            pos: 0,
        }
    }
}

impl std::io::Read for VecRead {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Write;

        let to_write = std::cmp::min(buf.len(), self.vec.len() - self.pos);
        if to_write > 0 {
            let written = buf.write(&self.vec[self.pos..to_write])?;
            self.pos += to_write;
            Ok(written)
        } else {
            Ok(0)
        }
    }
}

struct MapKeyDeserializer<'de> {
    key: Cow<'de, str>,
}

impl<'de> From<String> for MapKeyDeserializer<'de> {
    fn from(value: String) -> Self {
        MapKeyDeserializer {
            key: Cow::Owned(value),
        }
    }
}

impl<'de> serde::Deserializer<'de> for MapKeyDeserializer<'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        MapKeyStrDeserializer::from(self.key).deserialize_any(visitor)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.key.as_ref() {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(serde_json::Error::invalid_type(
                Unexpected::Str(&self.key),
                &visitor,
            )),
        }
    }

    impl_deser_numeric_key!(deserialize_i8);
    impl_deser_numeric_key!(deserialize_i16);
    impl_deser_numeric_key!(deserialize_i32);
    impl_deser_numeric_key!(deserialize_i64);
    impl_deser_numeric_key!(deserialize_u8);
    impl_deser_numeric_key!(deserialize_u16);
    impl_deser_numeric_key!(deserialize_u32);
    impl_deser_numeric_key!(deserialize_u64);
    impl_deser_numeric_key!(deserialize_f64);
    impl_deser_numeric_key!(deserialize_f32);
    impl_deser_numeric_key!(deserialize_i128);
    impl_deser_numeric_key!(deserialize_u128);

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.key
            .into_deserializer()
            .deserialize_enum(name, variants, visitor)
    }

    forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct identifier ignored_any
    }
}

struct MapKeyStrDeserializer<'de> {
    key: Cow<'de, str>,
}

impl<'de> From<Cow<'de, str>> for MapKeyStrDeserializer<'de> {
    fn from(value: Cow<'de, str>) -> Self {
        Self { key: value }
    }
}

impl<'de> serde::Deserializer<'de> for MapKeyStrDeserializer<'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.key {
            Cow::Owned(s) => visitor.visit_string(s),
            Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_enum(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

impl<'de> EnumAccess<'de> for MapKeyStrDeserializer<'de> {
    type Variant = UnitEnum;
    type Error = serde_json::Error;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        Ok((seed.deserialize(self)?, UnitEnum))
    }
}

struct UnitEnum;

impl<'de> VariantAccess<'de> for UnitEnum {
    type Error = serde_json::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(serde_json::Error::invalid_type(
            Unexpected::UnitVariant,
            &"tuple variant",
        ))
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(serde_json::Error::invalid_type(
            Unexpected::UnitVariant,
            &"struct variant",
        ))
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        Err(serde_json::Error::invalid_type(
            Unexpected::UnitVariant,
            &"newtype variant",
        ))
    }
}

fn unexpected_value(v: &Value) -> Unexpected<'_> {
    match v {
        Value::Null => Unexpected::Unit,
        Value::Bool(b) => Unexpected::Bool(*b),
        Value::Number(n) => {
            if let Some(n) = n.as_u64() {
                Unexpected::Unsigned(n)
            } else if let Some(n) = n.as_i64() {
                Unexpected::Signed(n)
            } else if let Some(n) = n.as_f64() {
                Unexpected::Float(n)
            } else {
                Unexpected::Other("non-unsigned, non-signed, non-float number")
            }
        }
        Value::String(s) => Unexpected::Str(s),
        Value::Array(_) => Unexpected::Seq,
        Value::Object(_) => Unexpected::Map,
    }
}

/// Converts a [`serde_json::Value`], with optional binary payloads into an arbitrary data type.
///
/// # Arguments
///
/// - `value` - a [`serde_json::Value`], with any binary blobs replaced with socket.io placeholder
///             objects
/// - `binary_payloads` - a [`Vec`] of binary payloads, in the order specified by the `num` fields
///                       of the placeholder objecst in `value`
pub fn from_value<'de, 'a, T: serde::Deserialize<'de> + 'a>(
    value: Value,
    binary_payloads: &'a [Bytes],
) -> Result<T, serde_json::Error> {
    let payload = Deserializer {
        value,
        binary_payloads,
        _phantom: PhantomData::<T>,
    };

    T::deserialize(payload)
}
