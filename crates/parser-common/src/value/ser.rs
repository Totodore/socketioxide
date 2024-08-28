use std::{fmt, io};

use bytes::Bytes;
use serde::ser::Impossible;
use serde_json::{ser::CompactFormatter, Serializer as JsonSerializer};
pub struct Serializer<'a, W> {
    event: Option<&'a str>,
    inner: JsonSerializer<W>,
    binary_payloads: Vec<Bytes>,
    binary_payloads_index: u32,
    is_root: bool,
}

impl<'a, W: io::Write> Serializer<'a, W> {
    pub fn new(writer: W, event: Option<&'a str>) -> Self {
        let inner = JsonSerializer::new(writer);
        Self {
            inner,
            event,
            binary_payloads: Vec::new(),
            binary_payloads_index: 0,
            is_root: true,
        }
    }
    pub fn into_binary(self) -> Vec<Bytes> {
        self.binary_payloads
    }
}

impl<'b, W: io::Write> Serializer<'b, W> {
    /// Serialize a tuple with an event and the beginning of the sequence:
    ///
    /// `[event, ...tuple]` or `[...tuple]`
    fn serialize_tuple_with_event<'a>(
        &'a mut self,
        len: Option<usize>,
    ) -> Result<serde_json::ser::Compound<'a, W, CompactFormatter>, serde_json::Error> {
        use serde::ser::{SerializeSeq, Serializer};
        if self.is_root {
            self.is_root = false;
            if let Some(e) = self.event {
                let mut seq = self.inner.serialize_seq(len.map(|l| l + 1))?;
                seq.serialize_element(&e)?;
                Ok(seq)
            } else {
                self.inner.serialize_seq(len)
            }
        } else {
            self.inner.serialize_seq(len)
        }
    }
}
impl<'a, 'b, W: io::Write> serde::Serializer for &'a mut Serializer<'b, W> {
    type Ok = <&'a mut JsonSerializer<W> as serde::Serializer>::Ok;

    type Error = <&'a mut JsonSerializer<W> as serde::Serializer>::Error;

    type SerializeSeq = <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeSeq;

    type SerializeTuple = <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeTuple;

    type SerializeTupleStruct =
        <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeTupleStruct;

    type SerializeTupleVariant =
        <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeTupleVariant;

    type SerializeMap = <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeMap;

    type SerializeStruct = <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeStruct;

    type SerializeStructVariant =
        <&'a mut JsonSerializer<W> as serde::Serializer>::SerializeStructVariant;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_bool(v)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i8(v)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i16(v)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i32(v)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i64(v)
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u8(v)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u16(v)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u32(v)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u64(v)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_f32(v)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_f64(v)
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_char(v)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_str(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        use serde::ser::SerializeMap;

        let num = self.binary_payloads_index;
        self.binary_payloads_index += 1;
        self.binary_payloads.push(Bytes::copy_from_slice(v)); //TODO: avoid copy ?

        let mut map = self.inner.serialize_map(Some(2))?;
        map.serialize_entry("_placeholder", &true)?;
        map.serialize_entry("num", &num)?;
        map.end()?;
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_none()
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.inner.serialize_some(value)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner
            .serialize_unit_variant(name, variant_index, variant)
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.inner.serialize_newtype_struct(name, value)
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        self.inner
            .serialize_newtype_variant(name, variant_index, variant, value)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.inner.serialize_seq(len)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_tuple_with_event(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_tuple_with_event(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.inner
            .serialize_tuple_variant(name, variant_index, variant, len)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.inner.serialize_map(len)
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.inner.serialize_struct(name, len)
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.inner
            .serialize_struct_variant(name, variant_index, variant, len)
    }
}

struct IsTupleSerializer;
#[derive(Debug)]
struct IsTupleSerializerError(bool);
impl fmt::Display for IsTupleSerializerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IsTupleSerializerError: {}", self.0)
    }
}
impl std::error::Error for IsTupleSerializerError {}
impl serde::ser::Error for IsTupleSerializerError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        IsTupleSerializerError(false)
    }
}

impl serde::Serializer for IsTupleSerializer {
    type Ok = bool;
    type Error = IsTupleSerializerError;
    type SerializeSeq = Impossible<bool, IsTupleSerializerError>;
    type SerializeTuple = Impossible<bool, IsTupleSerializerError>;
    type SerializeTupleStruct = Impossible<bool, IsTupleSerializerError>;
    type SerializeTupleVariant = Impossible<bool, IsTupleSerializerError>;
    type SerializeMap = Impossible<bool, IsTupleSerializerError>;
    type SerializeStruct = Impossible<bool, IsTupleSerializerError>;
    type SerializeStructVariant = Impossible<bool, IsTupleSerializerError>;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_some<T>(self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Ok(false)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(false)
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Ok(true)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Ok(false)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(IsTupleSerializerError(false))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(IsTupleSerializerError(true))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(IsTupleSerializerError(true))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(IsTupleSerializerError(false))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(IsTupleSerializerError(false))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(IsTupleSerializerError(false))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(IsTupleSerializerError(false))
    }
}

pub fn is_tuple<T: serde::Serialize>(value: &T) -> bool {
    match value.serialize(IsTupleSerializer) {
        Ok(v) | Err(IsTupleSerializerError(v)) => v,
    }
}
