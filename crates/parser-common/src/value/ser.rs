use std::{cell::RefCell, fmt, io, rc::Rc};

use bytes::Bytes;
use serde::ser::{
    Impossible, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};
use serde_json::{
    ser::{CompactFormatter, Compound as JsonCompound, Formatter, State},
    Error, Serializer as JsonSerializer,
};
pub struct Serializer<'event, W> {
    event: Option<&'event str>,
    inner: JsonSerializer<SharedWriter<W>>,
    writer: SharedWriter<W>,
    binary_payloads: Vec<Bytes>,
    binary_payloads_index: u32,
    is_root: bool,
}

#[derive(Debug)]
struct SharedWriter<W>(Rc<RefCell<W>>);
impl<W> Clone for SharedWriter<W> {
    fn clone(&self) -> Self {
        SharedWriter(self.0.clone())
    }
}
impl<W: io::Write> io::Write for SharedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

/// The Compound type is used to serialize map-like or vec-like structures.
/// It theoretically wraps the [`JsonCompound`] type from `serde_json` but allow to serialize elements
/// with the current [`Serializer`] (so we can handle binary payloads).
///
/// We can't keep an inner [`JsonCompound`] type because of lifetimes requirements on the serializer.
pub struct Compound<'a, 'event, W: io::Write> {
    ser: &'a mut Serializer<'event, W>,
    state: State,
}
type CompoundOutput<'c, W> = Result<JsonCompound<'c, SharedWriter<W>, CompactFormatter>, Error>;
impl<'a, 'event, W: io::Write> Compound<'a, 'event, W> {
    /// Creates a new [`Compound`] type with the current [`Serializer`]
    /// and a closure that returns a [`JsonCompound`] (the inner type).
    fn new(
        ser: &'a mut Serializer<'event, W>,
        cstr: impl for<'c> FnOnce(&'c mut Serializer<'event, W>) -> CompoundOutput<'c, W>,
    ) -> Result<Self, Error> {
        let state = match cstr(ser)? {
            JsonCompound::Map { state, .. } => state,
            _ => unreachable!(),
        };
        Ok(Self { state, ser })
    }

    /// Consumes the `Compound` and returns the inner `JsonCompound`.
    /// Mostly used when [`end`] is called and we don't need custom behavior.
    fn into_inner(self) -> JsonCompound<'a, SharedWriter<W>, CompactFormatter> {
        JsonCompound::Map {
            state: match self.state {
                State::Empty => State::Empty,
                State::First => State::First,
                State::Rest => State::Rest,
            },
            ser: &mut (*self.ser).inner,
        }
    }

    /// Executes the given closure with the inner `JsonCompound`.
    ///
    /// Mostly used when only need to use the inner `JsonCompound` for a short time.
    /// The state will be updated after the closure is executed.
    fn exec_inner<'c>(
        &'c mut self,
        f: impl FnOnce(&mut JsonCompound<'c, SharedWriter<W>, CompactFormatter>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut compound = JsonCompound::Map {
            state: match self.state {
                State::Empty => State::Empty,
                State::First => State::First,
                State::Rest => State::Rest,
            },
            ser: &mut (*self.ser).inner,
        };
        f(&mut compound)?;
        self.state = match compound {
            JsonCompound::Map { state, .. } => state,
            _ => unreachable!(),
        };
        Ok(())
    }
}

impl<W: io::Write> SerializeSeq for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        CompactFormatter
            .begin_array_value(&mut self.ser.writer, self.state == State::First)
            .map_err(Error::io)?;
        self.state = State::Rest;
        value.serialize(&mut *self.ser)?;
        CompactFormatter
            .end_array_value(&mut self.ser.writer)
            .map_err(Error::io)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self.into_inner())
    }
}
impl<W: io::Write> SerializeTuple for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}
impl<W: io::Write> SerializeTupleStruct for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}
impl<W: io::Write> SerializeTupleVariant for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self.into_inner())
    }
}
impl<W: io::Write> SerializeMap for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;
    #[inline]
    fn serialize_key<T>(&mut self, key: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        // We defer the serialization of the key to the inner `JsonCompound`
        // as we don't need to handle binary payloads for struct/map keys.
        self.exec_inner(|c| SerializeMap::serialize_key(c, key))
    }

    #[inline]
    fn serialize_value<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        CompactFormatter
            .begin_object_value(&mut self.ser.writer)
            .map_err(Error::io)?;
        value.serialize(&mut *self.ser)?; // We serialize with our own serializer
        CompactFormatter
            .end_object_value(&mut self.ser.writer)
            .map_err(Error::io)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeMap::end(self.into_inner())
    }
}
impl<W: io::Write> SerializeStruct for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        SerializeMap::serialize_entry(self, key, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeMap::end(self.into_inner())
    }
}
impl<W: io::Write> SerializeStructVariant for Compound<'_, '_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        SerializeMap::serialize_entry(self, key, value)
    }

    #[inline]
    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeMap::end(self.into_inner())
    }
}

impl<'event, W: io::Write> Serializer<'event, W> {
    pub fn new(writer: W, event: Option<&'event str>) -> Self {
        let writer = SharedWriter(Rc::new(RefCell::new(writer)));
        let inner = JsonSerializer::new(writer.clone());
        Self {
            inner,
            writer,
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

impl<'a, 'event, W: io::Write> Serializer<'event, W> {
    /// Serialize a tuple with an event and the beginning of the sequence:
    ///
    /// `[event, ...tuple]` or `[...tuple]`
    fn serialize_tuple_with_event(
        &'a mut self,
        len: Option<usize>,
    ) -> Result<Compound<'a, 'event, W>, Error> {
        use serde::ser::{SerializeSeq, Serializer};
        if self.is_root {
            self.is_root = false;
            if let Some(e) = self.event.clone() {
                let mut seq = self.serialize_seq(len.map(|l| l + 1))?;
                SerializeSeq::serialize_element(&mut seq, &e)?;
                Ok(seq)
            } else {
                self.serialize_seq(len)
            }
        } else {
            self.serialize_seq(len)
        }
    }
}
impl<'a, 'event, W: io::Write> serde::Serializer for &'a mut Serializer<'event, W> {
    type Ok = ();

    type Error = Error;

    type SerializeSeq = Compound<'a, 'event, W>;

    type SerializeTuple = Compound<'a, 'event, W>;

    type SerializeTupleStruct = Compound<'a, 'event, W>;

    type SerializeTupleVariant = Compound<'a, 'event, W>;

    type SerializeMap = Compound<'a, 'event, W>;

    type SerializeStruct = Compound<'a, 'event, W>;

    type SerializeStructVariant = Compound<'a, 'event, W>;

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
        SerializeMap::end(map)?;
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
        Compound::new(self, move |ser| ser.inner.serialize_seq(len))
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
        Compound::new(self, move |ser| {
            ser.inner
                .serialize_tuple_variant(name, variant_index, variant, len)
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Compound::new(self, move |ser| ser.inner.serialize_map(len))
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Compound::new(self, move |ser| ser.inner.serialize_struct(name, len))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Compound::new(self, move |ser| {
            ser.inner
                .serialize_struct_variant(name, variant_index, variant, len)
        })
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
