use std::collections::HashMap;

use bytes::Bytes;
use serde::ser::{Error, Impossible, SerializeSeq};
use serde_json::Number;

use super::{AsJson, PayloadValue};

const KEY_STRING_ERROR: &str = "key must be a string";

#[derive(Default)]
struct Serializer {
    next_binary_payload_num: usize,
}

pub fn to_payload_value<T: serde::Serialize>(data: T) -> Result<PayloadValue, serde_json::Error> {
    let mut ser = Serializer::default();
    data.serialize(&mut ser)
}

impl serde::Serialize for PayloadValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PayloadValue::Null => serializer.serialize_unit(),
            PayloadValue::Bool(b) => serializer.serialize_bool(*b),
            PayloadValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    serializer.serialize_u64(u)
                } else if let Some(i) = n.as_i64() {
                    serializer.serialize_i64(i)
                } else if let Some(f) = n.as_f64() {
                    serializer.serialize_f64(f)
                } else {
                    Err(S::Error::custom("number is not representable"))
                }
            }
            PayloadValue::String(s) => serializer.serialize_str(s.as_str()),
            PayloadValue::Binary(_, bin) => serializer.serialize_bytes(bin),
            PayloadValue::Array(a) => {
                let mut seq = serializer.serialize_seq(Some(a.len()))?;
                for elem in a.iter() {
                    seq.serialize_element(elem)?;
                }
                seq.end()
            }
            PayloadValue::Object(o) => {
                use serde::ser::SerializeMap;

                let mut map = serializer.serialize_map(Some(o.len()))?;
                for (key, value) in o.iter() {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'a> serde::Serialize for AsJson<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0 {
            PayloadValue::Null => serializer.serialize_unit(),
            PayloadValue::Bool(b) => serializer.serialize_bool(*b),
            PayloadValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    serializer.serialize_u64(u)
                } else if let Some(i) = n.as_i64() {
                    serializer.serialize_i64(i)
                } else if let Some(f) = n.as_f64() {
                    serializer.serialize_f64(f)
                } else {
                    Err(S::Error::custom("number is not representable"))
                }
            }
            PayloadValue::String(s) => serializer.serialize_str(s.as_str()),
            PayloadValue::Binary(num, _) => {
                use serde::ser::SerializeMap;

                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry(&"_placeholder", &true)?;
                map.serialize_entry(&"num", num)?;
                map.end()
            }
            PayloadValue::Array(a) => {
                let mut seq = serializer.serialize_seq(Some(a.len()))?;
                for elem in a.iter() {
                    seq.serialize_element(&AsJson(elem))?;
                }
                seq.end()
            }
            PayloadValue::Object(o) => {
                use serde::ser::SerializeMap;

                let mut map = serializer.serialize_map(Some(o.len()))?;
                for (key, value) in o.iter() {
                    map.serialize_entry(key, &AsJson(value))?;
                }
                map.end()
            }
        }
    }
}

impl<'a> serde::Serializer for &'a mut Serializer {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    type SerializeSeq = SerializeVec<'a>;
    type SerializeTuple = SerializeVec<'a>;
    type SerializeTupleVariant = SerializeTupleVariant<'a>;
    type SerializeMap = SerializeMap<'a>;
    type SerializeStruct = SerializeMap<'a>;
    type SerializeStructVariant = SerializeStructVariant<'a>;
    type SerializeTupleStruct = SerializeVec<'a>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Number(v.into()))
    }

    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        if let Ok(v) = u64::try_from(v) {
            Ok(PayloadValue::Number(v.into()))
        } else if let Ok(v) = i64::try_from(v) {
            Ok(PayloadValue::Number(v.into()))
        } else {
            Err(serde_json::Error::custom("number out of range"))
        }
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Number(v.into()))
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        if let Ok(v) = u64::try_from(v) {
            Ok(PayloadValue::Number(v.into()))
        } else {
            Err(serde_json::Error::custom("number out of range"))
        }
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        if v.is_finite() {
            Ok(Number::from_f64(v as f64).map_or(PayloadValue::Null, PayloadValue::Number))
        } else {
            Ok(PayloadValue::Null)
        }
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(Number::from_f64(v).map_or(PayloadValue::Null, PayloadValue::Number))
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(String::from(v)))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(v.to_string()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        let num = self.next_binary_payload_num;
        self.next_binary_payload_num += 1;
        Ok(PayloadValue::Binary(num, Bytes::copy_from_slice(v)))
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + serde::Serialize>(
        self,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(SerializeVec {
            root_ser: self,
            elements: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(SerializeTupleVariant {
            root_ser: self,
            name: String::from(variant),
            elements: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(SerializeMap {
            root_ser: self,
            map: HashMap::with_capacity(len.unwrap_or(0)),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(SerializeStructVariant {
            root_ser: self,
            name: String::from(variant),
            map: HashMap::new(),
        })
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let mut obj = HashMap::new();
        obj.insert(name.to_string(), value.serialize(self)?);
        Ok(PayloadValue::Object(obj))
    }

    fn collect_str<T: ?Sized + std::fmt::Display>(
        self,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::String(value.to_string()))
    }
}

struct SerializeVec<'a> {
    root_ser: &'a mut Serializer,
    elements: Vec<PayloadValue>,
}

impl<'a> SerializeSeq for SerializeVec<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        let element = value.serialize(&mut *self.root_ser)?;
        self.elements.push(element);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(PayloadValue::Array(self.elements))
    }
}

impl<'a> serde::ser::SerializeTuple for SerializeVec<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl<'a> serde::ser::SerializeTupleStruct for SerializeVec<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

struct SerializeTupleVariant<'a> {
    root_ser: &'a mut Serializer,
    name: String,
    elements: Vec<PayloadValue>,
}

impl<'a> serde::ser::SerializeTupleVariant for SerializeTupleVariant<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        let element = value.serialize(&mut *self.root_ser)?;
        self.elements.push(element);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut obj = HashMap::new();
        obj.insert(self.name, PayloadValue::Array(self.elements));
        Ok(PayloadValue::Object(obj))
    }
}

struct SerializeMap<'a> {
    root_ser: &'a mut Serializer,
    map: HashMap<String, PayloadValue>,
    next_key: Option<String>,
}

impl<'a> serde::ser::SerializeMap for SerializeMap<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_key<T: ?Sized + serde::Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.next_key = Some(key.serialize(MapKeySerializer)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + serde::Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), Self::Error> {
        if let Some(key) = self.next_key.take() {
            self.map.insert(key, value.serialize(&mut *self.root_ser)?);
            Ok(())
        } else {
            panic!("serialize_value() called before serialize_key()");
        }
    }

    fn serialize_entry<K: ?Sized + serde::Serialize, V: ?Sized + serde::Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), Self::Error> {
        self.map.insert(
            key.serialize(MapKeySerializer)?,
            value.serialize(&mut *self.root_ser)?,
        );
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if self.map.len() == 2
            && self
                .map
                .get("_placeholder")
                .and_then(|p| p.as_bool())
                .unwrap_or(false)
        {
            if let Some(num) = self.map.get("num").and_then(|n| n.as_u64()) {
                return Ok(PayloadValue::Binary(num as usize, Bytes::new()));
            }
        }

        Ok(PayloadValue::Object(self.map))
    }
}

impl<'a> serde::ser::SerializeStruct for SerializeMap<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        serde::ser::SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        serde::ser::SerializeMap::end(self)
    }
}

struct SerializeStructVariant<'a> {
    root_ser: &'a mut Serializer,
    name: String,
    map: HashMap<String, PayloadValue>,
}

impl<'a> serde::ser::SerializeStructVariant for SerializeStructVariant<'a> {
    type Ok = PayloadValue;
    type Error = serde_json::Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.map
            .insert(key.to_string(), value.serialize(&mut *self.root_ser)?);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut obj = HashMap::new();
        obj.insert(self.name, PayloadValue::Object(self.map));
        Ok(PayloadValue::Object(obj))
    }
}

struct MapKeySerializer;

impl serde::Serializer for MapKeySerializer {
    type Ok = String;
    type Error = serde_json::Error;

    type SerializeSeq = Impossible<String, Self::Error>;
    type SerializeMap = Impossible<String, Self::Error>;
    type SerializeTuple = Impossible<String, Self::Error>;
    type SerializeStruct = Impossible<String, Self::Error>;
    type SerializeTupleStruct = Impossible<String, Self::Error>;
    type SerializeTupleVariant = Impossible<String, Self::Error>;
    type SerializeStructVariant = Impossible<String, Self::Error>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_i128(self, v: i128) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_u128(self, v: u128) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(v.to_string())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_some<T: ?Sized + serde::Serialize>(
        self,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }

    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(Self::Error::custom(KEY_STRING_ERROR))
    }
}
