use serde::{de, Deserialize};
use std::fmt;

macro_rules! unsupported_type {
    ($trait_fn:ident) => {
        fn $trait_fn<V>(self, _: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            Err(NsParamDeserializationError::UnsupportedType(
                std::any::type_name::<V::Value>(),
            ))
        }
    };
}

macro_rules! parse_single_value {
    ($trait_fn:ident, $visit_fn:ident, $ty:literal) => {
        fn $trait_fn<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            if self.params.len() != 1 {
                return Err(NsParamDeserializationError::WrongNumberOfParameters {
                    got: self.params.len(),
                    expected: 1,
                });
            }
            let value = self.params.iter().next().unwrap().1;
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

#[derive(Debug)]
pub enum NsParamDeserializationError {
    UnsupportedType(&'static str),
    ParseError {
        value: String,
        expected_type: &'static str,
    },
    WrongNumberOfParameters {
        got: usize,
        expected: usize,
    },
}
struct Deserializer<'de> {
    params: &'de matchit::Params<'de, 'de>,
}
struct ValueDeserializer<'de> {
    key: Option<&'de str>,
    value: &'de str,
}
struct SeqDeserializer<'de> {
    iter: matchit::ParamsIter<'de, 'de, 'de>,
}
struct MapDeserializer<'de> {
    params: &'de matchit::Params<'de, 'de>,
    key: Option<&'de str>,
    value: Option<&'de str>,
}
struct EnumDeserializer<'de> {
    value: &'de str,
}

pub fn from_params<'de, T: Deserialize<'de>>(
    params: &'de matchit::Params<'de, 'de>,
) -> Result<T, NsParamDeserializationError> {
    let mut deserializer = Deserializer { params };
    T::deserialize(deserializer)
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
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

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let t = self.params.iter().next();
        if self.params.len() != 1 {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.params.len(),
                expected: 1,
            });
        }
        visitor.visit_borrowed_str(&self.params.iter().next().unwrap().1)
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
            iter: self.params.iter(),
        })
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.params.len() < len {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.params.len(),
                expected: len,
            });
        }
        visitor.visit_seq(SeqDeserializer {
            iter: self.params.iter(),
        })
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.params.len() < len {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.params.len(),
                expected: len,
            });
        }
        visitor.visit_seq(SeqDeserializer {
            iter: self.params.iter(),
        })
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(MapDeserializer {
            params: self.params,
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
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.params.len() != 1 {
            return Err(NsParamDeserializationError::WrongNumberOfParameters {
                got: self.params.len(),
                expected: 1,
            });
        }

        visitor.visit_enum(EnumDeserializer {
            value: &self.params.iter().next().unwrap().1,
        })
    }
}
/// ==== impl ValueDeserializer ====
macro_rules! parse_value {
    ($trait_fn:ident, $visit_fn:ident, $ty:literal) => {
        fn $trait_fn<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: de::Visitor<'de>,
        {
            let v = self.value.parse().map_err(|_| {
                if let Some(key) = self.key.take() {
                    let kind = match key {
                        KeyOrIdx::Key(key) => ErrorKind::ParseErrorAtKey {
                            key: key.to_owned(),
                            value: self.value.as_str().to_owned(),
                            expected_type: $ty,
                        },
                        KeyOrIdx::Idx { idx: index, key: _ } => ErrorKind::ParseErrorAtIndex {
                            index,
                            value: self.value.as_str().to_owned(),
                            expected_type: $ty,
                        },
                    };
                    PathDeserializationError::new(kind)
                } else {
                    PathDeserializationError::new(ErrorKind::ParseError {
                        value: self.value.as_str().to_owned(),
                        expected_type: $ty,
                    })
                }
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
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_str(self.value)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.value.as_bytes())
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        struct PairDeserializer<'de> {
            key: Option<KeyOrIdx<'de>>,
            value: Option<&'de PercentDecodedStr>,
        }

        impl<'de> SeqAccess<'de> for PairDeserializer<'de> {
            type Error = PathDeserializationError;

            fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
            where
                T: DeserializeSeed<'de>,
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
            Err(PathDeserializationError::unsupported_type(type_name::<
                V::Value,
            >()))
        }
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(PathDeserializationError::unsupported_type(type_name::<
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
        V: Visitor<'de>,
    {
        Err(PathDeserializationError::unsupported_type(type_name::<
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
        V: Visitor<'de>,
    {
        Err(PathDeserializationError::unsupported_type(type_name::<
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
        V: Visitor<'de>,
    {
        visitor.visit_enum(EnumDeserializer { value: self.value })
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

/// ==== impl SeqDeserializer ====
impl<'de> de::SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = NsParamDeserializationError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        self.iter.next().map_or(Ok(None), |(_, value)| {
            seed.deserialize(Deserializer {
                params: &mut [("", value)],
            })
            .map(Some)
        })
    }
}
/// ==== impl MapDeserializer ====

/// ==== impl EnumDeserializer ====

/// ==== impl NsParamDeserializationError ====

impl fmt::Display for NsParamDeserializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NsParamDeserializationError::UnsupportedType(t) => {
                write!(f, "Unsupported type: {}", t)
            }
            NsParamDeserializationError::ParseError {
                value,
                expected_type,
            } => {
                write!(
                    f,
                    "Failed to parse value: '{}', expected type: {}",
                    value, expected_type
                )
            }
            NsParamDeserializationError::WrongNumberOfParameters { got, expected } => {
                write!(
                    f,
                    "Wrong number of parameters, got: {}, expected: {}",
                    got, expected
                )
            }
        }
    }
}
impl std::error::Error for NsParamDeserializationError {}
impl de::Error for NsParamDeserializationError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        NsParamDeserializationError::ParseError {
            value: msg.to_string(),
            expected_type: "unknown",
        }
    }
}
