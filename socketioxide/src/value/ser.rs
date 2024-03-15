use bytes::Bytes;
use serde::ser::{Error, Impossible, SerializeSeq};
use serde_json::{Map, Value};

const KEY_STRING_ERROR: &str = "key must be a string";

macro_rules! forward_ser_impl {
    ($method:ident, $ty:ty) => {
        fn $method(self, v: $ty) -> Result<Self::Ok, Self::Error> {
            serde_json::value::Serializer.$method(v)
        }
    };
}

#[derive(Default)]
struct Serializer {
    binary_payloads: Vec<(usize, Bytes)>,
    next_binary_payload_num: usize,
}

impl<'a> serde::Serializer for &'a mut Serializer {
    type Ok = Value;
    type Error = serde_json::Error;

    type SerializeSeq = SerializeVec<'a>;
    type SerializeTuple = SerializeVec<'a>;
    type SerializeTupleVariant = SerializeTupleVariant<'a>;
    type SerializeMap = SerializeMap<'a>;
    type SerializeStruct = SerializeMap<'a>;
    type SerializeStructVariant = SerializeStructVariant<'a>;
    type SerializeTupleStruct = SerializeVec<'a>;

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        serde_json::value::Serializer.serialize_unit()
    }

    forward_ser_impl!(serialize_bool, bool);
    forward_ser_impl!(serialize_i8, i8);
    forward_ser_impl!(serialize_u8, u8);
    forward_ser_impl!(serialize_i16, i16);
    forward_ser_impl!(serialize_u16, u16);
    forward_ser_impl!(serialize_i32, i32);
    forward_ser_impl!(serialize_u32, u32);
    forward_ser_impl!(serialize_i64, i64);
    forward_ser_impl!(serialize_u64, u64);
    forward_ser_impl!(serialize_i128, i128);
    forward_ser_impl!(serialize_u128, u128);
    forward_ser_impl!(serialize_f32, f32);
    forward_ser_impl!(serialize_f64, f64);
    forward_ser_impl!(serialize_char, char);
    forward_ser_impl!(serialize_str, &str);

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        let num = self.next_binary_payload_num;
        self.next_binary_payload_num += 1;
        self.binary_payloads.push((num, Bytes::copy_from_slice(v)));
        Ok(Value::Object(
            [
                ("_placeholder".to_string(), Value::Bool(true)),
                ("num".to_string(), Value::Number(num.into())),
            ]
            .into_iter()
            .collect(),
        ))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        serde_json::value::Serializer.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        serde_json::value::Serializer.serialize_unit_variant(name, variant_index, variant)
    }

    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        serde_json::value::Serializer.serialize_none()
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

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(SerializeMap {
            root_ser: self,
            map: Map::new(),
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
            map: Map::new(),
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
        let mut obj = Map::new();
        obj.insert(name.to_string(), value.serialize(self)?);
        Ok(Value::Object(obj))
    }

    fn collect_str<T: ?Sized + std::fmt::Display>(
        self,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(Value::String(value.to_string()))
    }
}

struct SerializeVec<'a> {
    root_ser: &'a mut Serializer,
    elements: Vec<Value>,
}

impl<'a> SerializeSeq for SerializeVec<'a> {
    type Ok = Value;
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
        Ok(Value::Array(self.elements))
    }
}

impl<'a> serde::ser::SerializeTuple for SerializeVec<'a> {
    type Ok = Value;
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
    type Ok = Value;
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
    elements: Vec<Value>,
}

impl<'a> serde::ser::SerializeTupleVariant for SerializeTupleVariant<'a> {
    type Ok = Value;
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
        let mut obj = Map::new();
        obj.insert(self.name, Value::Array(self.elements));
        Ok(Value::Object(obj))
    }
}

struct SerializeMap<'a> {
    root_ser: &'a mut Serializer,
    map: Map<String, Value>,
    next_key: Option<String>,
}

impl<'a> serde::ser::SerializeMap for SerializeMap<'a> {
    type Ok = Value;
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
        Ok(Value::Object(self.map))
    }
}

impl<'a> serde::ser::SerializeStruct for SerializeMap<'a> {
    type Ok = Value;
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
    map: Map<String, Value>,
}

impl<'a> serde::ser::SerializeStructVariant for SerializeStructVariant<'a> {
    type Ok = Value;
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
        let mut obj = Map::new();
        obj.insert(self.name, Value::Object(self.map));
        Ok(Value::Object(obj))
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

/// Converts an arbitrary data type to a [`serde_json::Value`], extracting any binary payloads.
///
/// Binary data should be represented using the type [`bytes::Bytes`], or any other type that
/// implements [`serde::Serialize`] by serializing to bytes.  You can also use
/// `#[serde(serialize_with = "...")]` on a custom or existing type.
///
/// # Arguments
///
/// - `data` - a data type that implements [`serde::Serialize`]
pub fn to_value<T: serde::Serialize>(data: T) -> Result<(Value, Vec<Bytes>), serde_json::Error> {
    let mut ser = Serializer::default();
    let value = data.serialize(&mut ser)?;

    ser.binary_payloads
        .sort_by(|(num_a, _), (num_b, _)| num_a.cmp(num_b));

    Ok((
        value,
        ser.binary_payloads
            .into_iter()
            .map(|(_, bin)| bin)
            .collect(),
    ))
}
