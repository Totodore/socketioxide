//! `Deserializer` implementation for `NsParamBuff`

use serde::{de, forward_to_deserialize_any};
use std::iter::{ExactSizeIterator, Iterator};
use std::{any::type_name, fmt};

use super::NsParamBuff;

macro_rules! unsupported_type {
    ($trait_fn:ident) => {
        fn $trait_fn<V>(self, _: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            Err(NsParamDeserializationError::UnsupportedType(type_name::<
                V::Value,
            >()))
        }
    };
}

macro_rules! parse_single_value {
    ($trait_fn:ident, $visit_fn:ident, $ty:literal) => {
        fn $trait_fn<V: de::Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Self::Error> {
            if self.iter.len() != 1 {
                return Err(NsParamDeserializationError::WrongNumberOfParameters {
                    got: self.iter.len(),
                    expected: 1,
                });
            }
            let value = self.iter.next().unwrap().1;
            let value = value
                .parse()
                .map_err(|_| NsParamDeserializationError::ParseError {
                    value: value.to_owned(),
                    expected_type: $ty,
                })?;
            visitor.$visit_fn(value)
        }
    };
}

#[derive(Debug, PartialEq)]
pub enum NsParamDeserializationError {
    UnsupportedType(&'static str),
    Message(String),
    /// Failed to parse a value into the expected type.
    ///
    /// This variant is used when deserializing into a primitive type (such as `String` and `u32`).
    ParseError {
        value: String,
        expected_type: &'static str,
    },

    /// Failed to parse the value at a specific key into the expected type.
    ///
    /// This variant is used when deserializing into types that have named fields, such as structs.
    ParseErrorAtKey {
        /// The key at which the value was located.
        key: String,
        /// The value from the URI.
        value: String,
        /// The expected type of the value.
        expected_type: &'static str,
    },

    /// Failed to parse the value at a specific index into the expected type.
    ///
    /// This variant is used when deserializing into sequence types, such as tuples.
    ParseErrorAtIndex {
        /// The index at which the value was located.
        index: usize,
        /// The value from the URI.
        value: String,
        /// The expected type of the value.
        expected_type: &'static str,
    },
    WrongNumberOfParameters {
        got: usize,
        expected: usize,
    },
}

struct Deserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    iter: I,
}
struct ValueDeserializer<'de> {
    key: Option<KeyOrIdx<'de>>,
    value: &'de str,
}
struct SeqDeserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    iter: I,
    idx: usize,
}
struct MapDeserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    iter: I,
    key: Option<KeyOrIdx<'de>>,
    value: Option<&'de str>,
}
struct EnumDeserializer<'de> {
    value: &'de str,
}
struct KeyDeserializer<'de> {
    key: &'de str,
}
struct UnitVariant;

#[derive(Debug, Clone)]
enum KeyOrIdx<'de> {
    Key(&'de str),
    Idx { idx: usize, key: &'de str },
}

pub fn from_params<T: de::DeserializeOwned>(
    params: &NsParamBuff<'_>,
) -> Result<T, NsParamDeserializationError> {
    let deserializer = Deserializer {
        iter: params.into_iter().map(|(k, v)| (k.as_ref(), *v)),
    };
    T::deserialize(deserializer)
}

impl<'de, I> de::Deserializer<'de> for Deserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    type Error = NsParamDeserializationError;

    unsupported_type!(deserialize_bytes);
    unsupported_type!(deserialize_option);
    unsupported_type!(deserialize_identifier);
    unsupported_type!(deserialize_ignored_any);

    parse_single_value!(deserialize_bool, visit_bool, "bool");
    parse_single_value!(deserialize_i8, visit_i8, "i8");
    parse_single_value!(deserialize_i16, visit_i16, "i16");
    parse_single_value!(deserialize_i32, visit_i32, "i32");
    parse_single_value!(deserialize_i64, visit_i64, "i64");
    parse_single_value!(deserialize_i128, visit_i128, "i128");
    parse_single_value!(deserialize_u8, visit_u8, "u8");
    parse_single_value!(deserialize_u16, visit_u16, "u16");
    parse_single_value!(deserialize_u32, visit_u32, "u32");
    parse_single_value!(deserialize_u64, visit_u64, "u64");
    parse_single_value!(deserialize_u128, visit_u128, "u128");
    parse_single_value!(deserialize_f32, visit_f32, "f32");
    parse_single_value!(deserialize_f64, visit_f64, "f64");
    parse_single_value!(deserialize_string, visit_string, "String");
    parse_single_value!(deserialize_byte_buf, visit_string, "String");
    parse_single_value!(deserialize_char, visit_char, "char");

    fn deserialize_any<V: de::Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(v)
    }

    fn deserialize_str<V: de::Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.iter.len() != 1 {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.iter.len(),
                expected: 1,
            });
        }
        visitor.visit_borrowed_str(self.iter.next().unwrap().1)
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(SeqDeserializer {
            iter: self.iter,
            idx: 0,
        })
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        dbg!(&self.iter.len(), len);
        if self.iter.len() < len {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.iter.len(),
                expected: len,
            });
        }
        visitor.visit_seq(SeqDeserializer {
            iter: self.iter,
            idx: 0,
        })
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.iter.len() < len {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.iter.len(),
                expected: len,
            });
        }
        visitor.visit_seq(SeqDeserializer {
            iter: self.iter,
            idx: 0,
        })
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(MapDeserializer {
            iter: self.iter,
            value: None,
            key: None,
        })
    }

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        mut self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.iter.len() != 1 {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.iter.len(),
                expected: 1,
            });
        }

        visitor.visit_enum(EnumDeserializer {
            value: self.iter.next().unwrap().1,
        })
    }
}
/// ==== impl ValueDeserializer ====
macro_rules! parse_value {
    ($trait_fn:ident, $visit_fn:ident, $ty:literal) => {
        fn $trait_fn<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            let v = self.value.parse().map_err(|_| match self.key {
                Some(KeyOrIdx::Key(key)) => NsParamDeserializationError::ParseErrorAtKey {
                    value: self.value.to_owned(),
                    expected_type: $ty,
                    key: key.to_owned(),
                },
                Some(KeyOrIdx::Idx { idx, .. }) => NsParamDeserializationError::ParseErrorAtIndex {
                    value: self.value.to_owned(),
                    expected_type: $ty,
                    index: idx,
                },
                None => NsParamDeserializationError::ParseError {
                    value: self.value.to_owned(),
                    expected_type: $ty,
                },
            })?;
            visitor.$visit_fn(v)
        }
    };
}
impl<'de> de::Deserializer<'de> for ValueDeserializer<'de> {
    type Error = NsParamDeserializationError;
    unsupported_type!(deserialize_map);
    unsupported_type!(deserialize_identifier);

    parse_value!(deserialize_bool, visit_bool, "bool");
    parse_value!(deserialize_i8, visit_i8, "i8");
    parse_value!(deserialize_i16, visit_i16, "i16");
    parse_value!(deserialize_i32, visit_i32, "i32");
    parse_value!(deserialize_i64, visit_i64, "i64");
    parse_value!(deserialize_i128, visit_i128, "i128");
    parse_value!(deserialize_u8, visit_u8, "u8");
    parse_value!(deserialize_u16, visit_u16, "u16");
    parse_value!(deserialize_u32, visit_u32, "u32");
    parse_value!(deserialize_u64, visit_u64, "u64");
    parse_value!(deserialize_u128, visit_u128, "u128");
    parse_value!(deserialize_f32, visit_f32, "f32");
    parse_value!(deserialize_f64, visit_f64, "f64");
    parse_value!(deserialize_string, visit_string, "String");
    parse_value!(deserialize_byte_buf, visit_string, "String");
    parse_value!(deserialize_char, visit_char, "char");

    fn deserialize_any<V: de::Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(v)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_borrowed_str(self.value)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.value.as_bytes())
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
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

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        struct PairDeserializer<'de> {
            key: Option<KeyOrIdx<'de>>,
            value: Option<&'de str>,
        }

        impl<'de> de::SeqAccess<'de> for PairDeserializer<'de> {
            type Error = NsParamDeserializationError;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
            where
                T: de::DeserializeSeed<'de>,
            {
                match self.key.take() {
                    Some(KeyOrIdx::Idx { idx: _, key }) => {
                        return seed.deserialize(KeyDeserializer { key }).map(Some);
                    }
                    // `KeyOrIdx::Key` is only used when deserializing maps so `deserialize_seq`
                    // wouldn't be called for that
                    Some(KeyOrIdx::Key(_)) => unreachable!(),
                    None => {}
                };

                self.value
                    .take()
                    .map(|value| seed.deserialize(ValueDeserializer { key: None, value }))
                    .transpose()
            }
        }

        if len == 2 {
            match self.key {
                Some(key) => visitor.visit_seq(PairDeserializer {
                    key: Some(key),
                    value: Some(self.value),
                }),
                // `self.key` is only `None` when deserializing maps so `deserialize_seq`
                // wouldn't be called for that
                None => unreachable!(),
            }
        } else {
            Err(NsParamDeserializationError::UnsupportedType(type_name::<
                V::Value,
            >(
            )))
        }
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(NsParamDeserializationError::UnsupportedType(type_name::<
            V::Value,
        >()))
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(NsParamDeserializationError::UnsupportedType(type_name::<
            V::Value,
        >()))
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(NsParamDeserializationError::UnsupportedType(type_name::<
            V::Value,
        >()))
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
        visitor.visit_enum(EnumDeserializer { value: self.value })
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

/// ==== impl SeqDeserializer ====
impl<'de, I> de::SeqAccess<'de> for SeqDeserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    type Error = NsParamDeserializationError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        self.iter.next().map_or(Ok(None), |(key, value)| {
            let idx = self.idx;
            self.idx += 1;
            seed.deserialize(ValueDeserializer {
                key: Some(KeyOrIdx::Idx { key, idx }),
                value,
            })
            .map(Some)
        })
    }
}

/// ==== impl MapDeserializer ====
impl<'de, I> de::MapAccess<'de> for MapDeserializer<'de, I>
where
    I: Iterator<Item = (&'de str, &'de str)> + ExactSizeIterator,
{
    type Error = NsParamDeserializationError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                self.key = Some(KeyOrIdx::Key(key));
                seed.deserialize(KeyDeserializer { key }).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(value) => seed.deserialize(ValueDeserializer {
                key: self.key.take(),
                value,
            }),
            None => Err(serde::de::Error::custom("value is missing")),
        }
    }
}

/// ==== impl EnumDeserializer ====
impl<'de> de::EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = NsParamDeserializationError;
    type Variant = UnitVariant;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        Ok((
            seed.deserialize(KeyDeserializer { key: self.value })?,
            UnitVariant,
        ))
    }
}

/// ==== impl UnitVariant ====

impl<'de> de::VariantAccess<'de> for UnitVariant {
    type Error = NsParamDeserializationError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, _seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        Err(NsParamDeserializationError::UnsupportedType(
            "newtype enum variant",
        ))
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(NsParamDeserializationError::UnsupportedType(
            "tuple enum variant",
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
        Err(NsParamDeserializationError::UnsupportedType(
            "struct enum variant",
        ))
    }
}

/// ==== impl KeyDeserializer ====
macro_rules! parse_key {
    ($trait_fn:ident) => {
        fn $trait_fn<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            visitor.visit_str(&self.key)
        }
    };
}

impl<'de> de::Deserializer<'de> for KeyDeserializer<'de> {
    type Error = NsParamDeserializationError;

    parse_key!(deserialize_identifier);
    parse_key!(deserialize_str);
    parse_key!(deserialize_string);

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        Err(de::Error::custom("Unexpected key type"))
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char bytes
        byte_buf option unit unit_struct seq tuple
        tuple_struct map newtype_struct struct enum ignored_any
    }
}

/// ==== impl NsParamDeserializationError ====

impl fmt::Display for NsParamDeserializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use NsParamDeserializationError::*;
        match self {
            UnsupportedType(t) => write!(f, "Unsupported type: {}", t),
            ParseError {
                value,
                expected_type,
            } => {
                write!(
                    f,
                    "Failed to parse value: '{}', expected type: {}",
                    value, expected_type
                )
            }
            ParseErrorAtKey {
                value,
                expected_type,
                key,
            } => {
                write!(
                    f,
                    "Failed to parse value: '{}', expected type: {} at key: {}",
                    value, expected_type, key
                )
            }
            ParseErrorAtIndex {
                value,
                expected_type,
                index,
            } => {
                write!(
                    f,
                    "Failed to parse value: '{}', expected type: {} at index: {}",
                    value, expected_type, index
                )
            }
            WrongNumberOfParameters { got, expected } => {
                write!(
                    f,
                    "Wrong number of parameters, got: {}, expected: {}",
                    got, expected
                )
            }
            Message(msg) => write!(f, "{}", msg),
        }
    }
}
impl std::error::Error for NsParamDeserializationError {}
impl serde::de::Error for NsParamDeserializationError {
    #[inline]
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        Self::Message(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    enum MyEnum {
        A,
        B,
        #[serde(rename = "c")]
        C,
    }

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    struct Struct {
        c: String,
        b: bool,
        a: i32,
    }

    macro_rules! check_single_value {
        ($ty:ty, $value_str:literal, $value:expr) => {
            #[allow(clippy::bool_assert_comparison)]
            {
                let iter = [("value", $value_str)].into_iter();
                let deserializer = Deserializer { iter };
                assert_eq!(<$ty>::deserialize(deserializer).unwrap(), $value);
            }
        };
    }

    #[test]
    fn test_parse_single_value() {
        check_single_value!(bool, "true", true);
        check_single_value!(bool, "false", false);
        check_single_value!(i8, "-123", -123);
        check_single_value!(i16, "-123", -123);
        check_single_value!(i32, "-123", -123);
        check_single_value!(i64, "-123", -123);
        check_single_value!(i128, "123", 123);
        check_single_value!(u8, "123", 123);
        check_single_value!(u16, "123", 123);
        check_single_value!(u32, "123", 123);
        check_single_value!(u64, "123", 123);
        check_single_value!(u128, "123", 123);
        check_single_value!(f32, "123", 123.0);
        check_single_value!(f64, "123", 123.0);
        check_single_value!(String, "abc", "abc");
        check_single_value!(String, "one two", "one two");
        check_single_value!(&str, "abc", "abc");
        check_single_value!(&str, "one two", "one two");
        check_single_value!(char, "a", 'a');

        let iter = [("a", "B")].into_iter();
        assert_eq!(
            MyEnum::deserialize(Deserializer { iter }).unwrap(),
            MyEnum::B
        );

        let iter = [("a", "1"), ("b", "2")].into_iter();
        let error_kind = i32::deserialize(Deserializer { iter }).unwrap_err();
        assert!(matches!(
            error_kind,
            NsParamDeserializationError::WrongNumberOfParameters {
                expected: 1,
                got: 2
            }
        ));
    }

    #[test]
    fn test_parse_seq() {
        let iter = [("a", "1"), ("b", "true"), ("c", "abc")].into_iter();
        assert_eq!(
            <(i32, bool, String)>::deserialize(Deserializer { iter: iter.clone() }).unwrap(),
            (1, true, "abc".to_owned())
        );

        #[derive(Debug, Deserialize, Eq, PartialEq)]
        struct TupleStruct(i32, bool, String);
        assert_eq!(
            TupleStruct::deserialize(Deserializer { iter }).unwrap(),
            TupleStruct(1, true, "abc".to_owned())
        );

        let iter = [("a", "1"), ("b", "2"), ("c", "3")].into_iter();
        assert_eq!(
            <Vec<i32>>::deserialize(Deserializer { iter }).unwrap(),
            vec![1, 2, 3]
        );

        let iter = [("a", "c"), ("a", "B")].into_iter();
        assert_eq!(
            <Vec<MyEnum>>::deserialize(Deserializer { iter }).unwrap(),
            vec![MyEnum::C, MyEnum::B]
        );
    }

    #[test]
    fn test_parse_seq_tuple_string_string() {
        let iter = [("a", "foo"), ("b", "bar")].into_iter();
        assert_eq!(
            <Vec<(String, String)>>::deserialize(Deserializer { iter }).unwrap(),
            vec![
                ("a".to_owned(), "foo".to_owned()),
                ("b".to_owned(), "bar".to_owned())
            ]
        );
    }

    #[test]
    fn test_parse_seq_tuple_string_parse() {
        let iter = vec![("a", "1"), ("b", "2")].into_iter();
        assert_eq!(
            <Vec<(String, u32)>>::deserialize(Deserializer { iter }).unwrap(),
            vec![("a".to_owned(), 1), ("b".to_owned(), 2)]
        );
    }

    #[test]
    fn test_parse_struct() {
        let iter = [("a", "1"), ("b", "true"), ("c", "abc")].into_iter();
        assert_eq!(
            Struct::deserialize(Deserializer { iter }).unwrap(),
            Struct {
                c: "abc".to_owned(),
                b: true,
                a: 1,
            }
        );
    }

    #[test]
    fn test_parse_struct_ignoring_additional_fields() {
        let iter = [("a", "1"), ("b", "true"), ("c", "abc"), ("d", "false")].into_iter();
        assert_eq!(
            Struct::deserialize(Deserializer { iter }).unwrap(),
            Struct {
                c: "abc".to_owned(),
                b: true,
                a: 1,
            }
        );
    }

    #[test]
    fn test_parse_tuple_ignoring_additional_fields() {
        let iter = [("a", "abc"), ("b", "true"), ("c", "1"), ("d", "false")].into_iter();
        assert_eq!(
            <(&str, bool, u32)>::deserialize(Deserializer { iter }).unwrap(),
            ("abc", true, 1)
        );
    }

    #[test]
    fn test_parse_map() {
        use std::collections::HashMap;
        let iter = [("a", "1"), ("b", "true"), ("c", "abc")].into_iter();
        assert_eq!(
            <HashMap<String, String>>::deserialize(Deserializer { iter }).unwrap(),
            [("a", "1"), ("b", "true"), ("c", "abc")]
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        );
    }

    macro_rules! test_parse_error {
        (
            $params:expr,
            $ty:ty,
            $expected_error_kind:expr $(,)?
        ) => {
            let iter = $params.into_iter();
            let err = <$ty>::deserialize(Deserializer { iter }).unwrap_err();
            assert_eq!(err, $expected_error_kind);
        };
    }

    #[test]
    fn test_wrong_number_of_parameters_error() {
        test_parse_error!(
            [("a", "1")],
            (u32, u32),
            NsParamDeserializationError::WrongNumberOfParameters {
                got: 1,
                expected: 2,
            }
        );
    }

    #[test]
    fn test_parse_error_at_key_error() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Params {
            a: u32,
        }
        test_parse_error!(
            [("a", "false")],
            Params,
            NsParamDeserializationError::ParseErrorAtKey {
                key: "a".to_owned(),
                value: "false".to_owned(),
                expected_type: "u32",
            }
        );
    }

    #[test]
    fn test_parse_error_at_key_error_multiple() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Params {
            a: u32,
            b: u32,
        }
        test_parse_error!(
            [("a", "false")],
            Params,
            NsParamDeserializationError::ParseErrorAtKey {
                key: "a".to_owned(),
                value: "false".to_owned(),
                expected_type: "u32",
            }
        );
    }

    #[test]
    fn test_parse_error_at_index_error() {
        test_parse_error!(
            [("a", "false"), ("b", "true")],
            (bool, u32),
            NsParamDeserializationError::ParseErrorAtIndex {
                index: 1,
                value: "true".to_owned(),
                expected_type: "u32",
            }
        );
    }

    #[test]
    fn test_parse_error_error() {
        test_parse_error!(
            [("a", "false")],
            u32,
            NsParamDeserializationError::ParseError {
                value: "false".to_owned(),
                expected_type: "u32",
            }
        );
    }

    #[test]
    fn test_unsupported_type_error_nested_data_structure() {
        test_parse_error!(
            [("a", "false")],
            Vec<Vec<u32>>,
            NsParamDeserializationError::UnsupportedType("alloc::vec::Vec<u32>")
        );
    }

    #[test]
    fn test_parse_seq_tuple_unsupported_key_type() {
        test_parse_error!(
            [("a", "false")],
            Vec<(u32, String)>,
            NsParamDeserializationError::Message("Unexpected key type".to_owned())
        );
    }

    #[test]
    fn test_parse_seq_wrong_tuple_length() {
        test_parse_error!(
            [("a", "false")],
            Vec<(String, String, String)>,
            NsParamDeserializationError::UnsupportedType(
                "(alloc::string::String, alloc::string::String, alloc::string::String)"
            ),
        );
    }

    #[test]
    fn test_parse_seq_seq() {
        test_parse_error!(
            [("a", "false")],
            Vec<Vec<String>>,
            NsParamDeserializationError::UnsupportedType("alloc::vec::Vec<alloc::string::String>"),
        );
    }
}
