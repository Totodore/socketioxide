use std::{borrow::Cow, collections::HashMap, marker::PhantomData, vec};

use bytes::Bytes;
use serde::{
    de::{
        self, DeserializeSeed, EnumAccess, Error, IntoDeserializer, MapAccess, SeqAccess,
        Unexpected, VariantAccess,
    },
    forward_to_deserialize_any,
};
use serde_json::Number;

use super::PayloadValue;

// For Deserializer impl for PayloadValue
macro_rules! impl_deser_number {
    ($method:ident) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            match self {
                PayloadValue::Number(n) => n.$method(visitor),
                _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
            }
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

macro_rules! impl_visit_integer {
    ($ty:ty, $fn:ident) => {
        fn $fn<E>(self, v: $ty) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(PayloadValue::Number(v.into()))
        }
    };
}

macro_rules! impl_visit_float {
    ($ty:ty, $fn:ident) => {
        fn $fn<E>(self, v: $ty) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Number::from_f64(v as f64)
                .map(PayloadValue::Number)
                .ok_or_else(|| {
                    E::custom(format!(
                        "float value '{}' could not be converted into a number",
                        v
                    ))
                })
        }
    };
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

impl PayloadValue {
    fn unexpected(&self) -> Unexpected<'_> {
        match self {
            PayloadValue::Null => Unexpected::Unit,
            PayloadValue::Bool(b) => Unexpected::Bool(*b),
            PayloadValue::Number(n) => {
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
            PayloadValue::String(s) => Unexpected::Str(s),
            PayloadValue::Binary(_, data) => Unexpected::Bytes(data),
            PayloadValue::Array(_) => Unexpected::Seq,
            PayloadValue::Object(_) => Unexpected::Map,
        }
    }
}

impl<'de> serde::Deserialize<'de> for PayloadValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let mut next_binary_payload_num: usize = 0;
        let state = PayloadValueState(&mut next_binary_payload_num);
        state.deserialize(deserializer)
    }
}

struct PayloadValueState<'a>(&'a mut usize);

impl<'de, 'a> serde::de::DeserializeSeed<'de> for PayloadValueState<'a> {
    type Value = PayloadValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct PayloadValueVisitor<'a>(&'a mut usize);

        impl<'de, 'a> de::Visitor<'de> for PayloadValueVisitor<'a> {
            type Value = PayloadValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a json value")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::Null)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::Null)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: de::Deserializer<'de>,
            {
                deserializer.deserialize_any(self)
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::Bool(v))
            }

            impl_visit_integer!(u8, visit_u8);
            impl_visit_integer!(i8, visit_i8);
            impl_visit_integer!(u16, visit_u16);
            impl_visit_integer!(i16, visit_i16);
            impl_visit_integer!(u32, visit_u32);
            impl_visit_integer!(i32, visit_i32);
            impl_visit_integer!(u64, visit_u64);
            impl_visit_integer!(i64, visit_i64);
            impl_visit_float!(f32, visit_f32);
            impl_visit_float!(f64, visit_f64);

            fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::String(v.into()))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::String(v.to_string()))
            }

            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::String(v.to_string()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::String(v))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::Binary(0, Bytes::copy_from_slice(v)))
            }

            fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(PayloadValue::Binary(0, Bytes::copy_from_slice(v)))
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let num = *self.0;
                *self.0 += 1;
                Ok(PayloadValue::Binary(num, v.into()))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut vec = Vec::<PayloadValue>::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(element) = seq.next_element_seed(PayloadValueState(self.0))? {
                    vec.push(element);
                }
                Ok(PayloadValue::Array(vec))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut hmap =
                    HashMap::<String, PayloadValue>::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, value)) =
                    map.next_entry_seed(PhantomData, PayloadValueState(self.0))?
                {
                    hmap.insert(key, value);
                }

                if hmap.len() == 2
                    && hmap
                        .get("_placeholder")
                        .and_then(|pv| pv.as_bool())
                        .unwrap_or(false)
                {
                    if let Some(num) = hmap.get("num").and_then(|pv| pv.as_u64()) {
                        return Ok(PayloadValue::Binary(num as usize, Bytes::new()));
                    }
                }

                Ok(PayloadValue::Object(hmap))
            }
        }

        deserializer.deserialize_any(PayloadValueVisitor(self.0))
    }
}

impl<'de> serde::Deserializer<'de> for PayloadValue {
    type Error = serde_json::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Null => visitor.visit_unit(),
            PayloadValue::Bool(b) => visitor.visit_bool(b),
            PayloadValue::Number(n) => n.deserialize_any(visitor),
            PayloadValue::String(s) => visitor.visit_string(s),
            PayloadValue::Binary(_, data) => visitor.visit_bytes(&data),
            PayloadValue::Array(a) => visit_array(a, visitor),
            PayloadValue::Object(o) => visit_object(o, visitor),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Null => visitor.visit_unit(),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Bool(b) => visitor.visit_bool(b),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
        }
    }

    impl_deser_number!(deserialize_i8);
    impl_deser_number!(deserialize_i16);
    impl_deser_number!(deserialize_i32);
    impl_deser_number!(deserialize_i64);
    impl_deser_number!(deserialize_i128);
    impl_deser_number!(deserialize_u8);
    impl_deser_number!(deserialize_u16);
    impl_deser_number!(deserialize_u32);
    impl_deser_number!(deserialize_u64);
    impl_deser_number!(deserialize_u128);
    impl_deser_number!(deserialize_f32);
    impl_deser_number!(deserialize_f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::String(s) => visitor.visit_string(s),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::String(s) => visitor.visit_string(s),
            PayloadValue::Binary(_, data) => visitor.visit_bytes(&data),
            PayloadValue::Array(a) => visit_array(a, visitor),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Array(a) => visit_array(a, visitor),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

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

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self {
            PayloadValue::Object(o) => visit_object(o, visitor),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
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
        match self {
            PayloadValue::Object(o) => visit_object(o, visitor),
            PayloadValue::Array(a) => visit_array(a, visitor),
            _ => Err(serde_json::Error::invalid_type(self.unexpected(), &visitor)),
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
        match self {
            PayloadValue::Object(o) => {
                // From serde_json: enums are encoded as a map with _only_ a single key-value pair
                if o.len() == 1 {
                    let (variant, value) = o.into_iter().next().unwrap();
                    visitor.visit_enum(EnumDeserializer::from((variant, Some(value))))
                } else {
                    Err(serde_json::Error::invalid_value(
                        Unexpected::Map,
                        &"a map with a single key-value pair",
                    ))
                }
            }
            PayloadValue::String(s) => visitor.visit_enum(EnumDeserializer::from((s, None))),
            _ => Err(serde_json::Error::invalid_type(
                self.unexpected(),
                &"map or string",
            )),
        }
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        drop(self);
        visitor.visit_unit()
    }
}

fn visit_array<'de, V>(a: Vec<PayloadValue>, visitor: V) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
{
    let len = a.len();
    let mut deser = SeqDeserializer::from(a);
    let seq = visitor.visit_seq(&mut deser)?;
    if deser.iter.len() == 0 {
        Ok(seq)
    } else {
        Err(serde_json::Error::invalid_length(
            len,
            &"array with fewer elements",
        ))
    }
}

fn visit_object<'de, V>(
    o: HashMap<String, PayloadValue>,
    visitor: V,
) -> Result<V::Value, serde_json::Error>
where
    V: de::Visitor<'de>,
{
    let len = o.len();
    let mut deser = MapDeserializer::from(o);
    let map = visitor.visit_map(&mut deser)?;
    if deser.iter.len() == 0 {
        Ok(map)
    } else {
        Err(serde_json::Error::invalid_length(
            len,
            &"map with fewer elements",
        ))
    }
}

struct SeqDeserializer {
    iter: vec::IntoIter<PayloadValue>,
}

impl From<Vec<PayloadValue>> for SeqDeserializer {
    fn from(value: Vec<PayloadValue>) -> Self {
        Self {
            iter: value.into_iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = serde_json::Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed.deserialize(value).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (_, Some(upper)) => Some(upper),
            (lower, _) if lower > 0 => Some(lower),
            _ => None,
        }
    }
}

struct MapDeserializer {
    iter: <HashMap<String, PayloadValue> as IntoIterator>::IntoIter,
    next_value: Option<PayloadValue>,
}

impl From<HashMap<String, PayloadValue>> for MapDeserializer {
    fn from(value: HashMap<String, PayloadValue>) -> Self {
        Self {
            iter: value.into_iter(),
            next_value: None,
        }
    }
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = serde_json::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.next_value = Some(value);
                let deser = MapKeyDeserializer::from(key);
                seed.deserialize(deser).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.next_value.take() {
            Some(value) => seed.deserialize(value),
            None => Err(serde_json::Error::custom(
                "next_value() called before next_key()",
            )),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.iter.size_hint() {
            (_, Some(upper)) => Some(upper),
            (lower, _) if lower > 0 => Some(lower),
            _ => None,
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

struct EnumDeserializer {
    variant: String,
    value: Option<PayloadValue>,
}

impl From<(String, Option<PayloadValue>)> for EnumDeserializer {
    fn from(value: (String, Option<PayloadValue>)) -> Self {
        Self {
            variant: value.0,
            value: value.1,
        }
    }
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Variant = VariantDeserializer;
    type Error = serde_json::Error;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let variant = self.variant.into_deserializer();
        let visitor = VariantDeserializer::from(self.value);
        seed.deserialize(variant).map(|v| (v, visitor))
    }
}

struct VariantDeserializer {
    value: Option<PayloadValue>,
}

impl From<Option<PayloadValue>> for VariantDeserializer {
    fn from(value: Option<PayloadValue>) -> Self {
        Self { value }
    }
}

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = serde_json::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            Some(value) => serde::Deserialize::deserialize(value),
            None => Ok(()),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Some(PayloadValue::Array(a)) => {
                if a.is_empty() {
                    visitor.visit_unit()
                } else {
                    visit_array(a, visitor)
                }
            }
            Some(other) => Err(serde_json::Error::invalid_type(
                other.unexpected(),
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
            Some(PayloadValue::Object(o)) => visit_object(o, visitor),
            Some(other) => Err(serde_json::Error::invalid_type(
                other.unexpected(),
                &"struct variant",
            )),
            None => Err(serde_json::Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(value),
            None => Err(serde_json::Error::invalid_type(
                Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }
}
