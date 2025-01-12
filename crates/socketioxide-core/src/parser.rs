//! Contains all the type and interfaces for the parser sub-crates.
//!
//! The parsing system in socketioxide is split in three phases for serialization and deserialization:
//!
//! ## Deserialization
//! * The SocketIO server receives a packet and calls [`Parse::decode_str`] or [`Parse::decode_bin`]
//!   to decode the incoming packet. If a [`ParseError::NeedsMoreBinaryData`] is returned, the server keeps the packet
//!   and waits for new incoming data.
//! * Once the packet is complete, the server dispatches the packet to the appropriate namespace
//!   and calls [`Parse::read_event`] to route the data to the appropriate event handler.
//! * If the user-provided handler has a `Data` extractor with a provided `T` type, the server
//!   calls [`Parse::decode_value`] to deserialize the data. This function will handle the event
//!   name as the first argument of the array, as well as variadic arguments.
//!
//! ## Serialization
//! * The user calls emit from the socket or namespace with custom `Serializable` data. The server calls
//!   [`Parse::encode_value`] to convert this to a raw [`Value`] to be included in the packet. The function
//!   will take care of handling the event name as the first argument of the array, along with variadic arguments.
//! * The server then calls [`Parse::encode`] to convert the payload provided by the user,
//!   along with other metadata (namespace, ack ID, etc.), into a fully serialized packet ready to be sent.

use std::{
    error::Error as StdError,
    fmt,
    sync::{atomic::AtomicUsize, Mutex},
};

use bytes::Bytes;
use engineioxide::Str;
use serde::{de::Visitor, ser::Impossible, Deserialize, Serialize};

use crate::{packet::Packet, Value};

/// The parser state that is shared between the parser and the socket.io system.
/// Used to handle partial binary packets when receiving binary packets that have
/// adjacent binary attachments
#[derive(Debug, Default)]
pub struct ParserState {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received for common parser
    pub partial_bin_packet: Mutex<Option<Packet>>,

    /// The number of expected binary attachments (used when receiving data for common parser)
    pub incoming_binary_cnt: AtomicUsize,
}

/// All socket.io parser should implement this trait.
/// Parsers should be stateless.
pub trait Parse: Default + Copy {
    /// Convert a packet into multiple payloads to be sent.
    fn encode(self, packet: Packet) -> Value;

    /// Parse a given input string. If the payload needs more adjacent binary packet,
    /// the partial packet will be kept and a [`ParseError::NeedsMoreBinaryData`] will be returned.
    fn decode_str(self, state: &ParserState, data: Str) -> Result<Packet, ParseError>;

    /// Parse a given input binary. If the payload needs more adjacent binary packet,
    /// the partial packet is still kept and a [`ParseError::NeedsMoreBinaryData`] will be returned.
    fn decode_bin(self, state: &ParserState, bin: Bytes) -> Result<Packet, ParseError>;

    /// Convert any serializable data to a generic [`Value`] to be later included as a payload in the packet.
    ///
    /// * Any data serialized will be wrapped in an array to match the socket.io format (`[data]`).
    /// * If the data is a tuple-like type the tuple will be expanded in the array (`[...data]`).
    /// * If provided the event name will be serialized as the first element of the array
    ///   (`[event, ...data]`) or (`[event, data]`).
    fn encode_value<T: ?Sized + Serialize>(
        self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, ParserError>;

    /// Convert any generic [`Value`] to a deserializable type.
    /// It should always be an array (according to the serde model).
    ///
    /// * If `with_event` is true, we expect an event str as the first element and we will skip
    ///   it when deserializing data.
    /// * If `T` is a tuple-like type, all the remaining elements of the array will be
    ///   deserialized (`[...data]`).
    /// * If `T` is not a tuple-like type, the first element of the array will be deserialized (`[data]`).
    fn decode_value<'de, T: Deserialize<'de>>(
        self,
        value: &'de mut Value,
        with_event: bool,
    ) -> Result<T, ParserError>;

    /// Convert any generic [`Value`] to a type with the default serde impl without binary + event tricks.
    /// This is mainly used for connect payloads.
    fn decode_default<'de, T: Deserialize<'de>>(
        self,
        value: Option<&'de Value>,
    ) -> Result<T, ParserError>;

    /// Convert any type to a generic [`Value`] with the default serde impl without binary + event tricks.
    /// This is mainly used for connect payloads.
    fn encode_default<T: ?Sized + Serialize>(self, data: &T) -> Result<Value, ParserError>;

    /// Try to read the event name from the given payload data.
    /// The event name should be the first element of the provided array according to the serde model.
    fn read_event(self, value: &Value) -> Result<&str, ParserError>;
}

/// A parser error that wraps any error that can occur during parsing.
///
/// E.g. `serde_json::Error`, `rmp_serde::Error`...
#[derive(Debug)]
pub struct ParserError {
    inner: Box<dyn StdError + Send + Sync + 'static>,
}
impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}
impl std::error::Error for ParserError {}
impl Serialize for ParserError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.inner.to_string().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ParserError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        #[derive(Debug, thiserror::Error)]
        #[error("remote err: {0:?}")]
        struct RemoteErr(String);

        Ok(Self::new(RemoteErr(s)))
    }
}
impl ParserError {
    /// Create a new parser error from any error that implements [`std::error::Error`]
    pub fn new<E: StdError + Send + Sync + 'static>(inner: E) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }
}
/// Errors when parsing/serializing socket.io packets
#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    /// Invalid packet type
    #[error("invalid packet type")]
    InvalidPacketType,

    /// Invalid ack id
    #[error("invalid ack id")]
    InvalidAckId,

    /// Invalid event name
    #[error("invalid event name")]
    InvalidEventName,

    /// Invalid data
    #[error("invalid data")]
    InvalidData,

    /// Invalid namespace
    #[error("invalid namespace")]
    InvalidNamespace,

    /// Invalid attachments
    #[error("invalid attachments")]
    InvalidAttachments,

    /// Received unexpected binary data
    #[error(
        "received unexpected binary data. Make sure you are using the same parser on both ends."
    )]
    UnexpectedBinaryPacket,

    /// Received unexpected string data
    #[error(
        "received unexpected string data. Make sure you are using the same parser on both ends."
    )]
    UnexpectedStringPacket,

    /// Needs more binary data before deserialization. It is not exactly an error, it is used for control flow,
    /// e.g the common parser needs adjacent binary packets and therefore will returns
    /// `NeedsMoreBinaryData` n times for n adjacent binary packet expected.
    ///
    /// In this case the user should call again the parser with the next binary payload.
    #[error("needs more binary data for packet completion")]
    NeedsMoreBinaryData,

    /// The inner parser error
    #[error("parser error: {0:?}")]
    ParserError(#[from] ParserError),
}

/// A seed that can be used to deserialize only the 1st element of a sequence
pub struct FirstElement<T>(std::marker::PhantomData<T>);
impl<T> Default for FirstElement<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}
impl<'de, T: serde::Deserialize<'de>> serde::de::Visitor<'de> for FirstElement<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "a sequence in which we care about first element",)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        use serde::de::Error;
        let data = seq
            .next_element::<T>()?
            .ok_or(A::Error::custom("first element not found"));

        // Consume the rest of the sequence
        while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}

        data
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::de::DeserializeSeed<'de> for FirstElement<T> {
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

/// Serializer and deserializer that simply return if the root object is a tuple or not.
/// It is used with [`is_de_tuple`] and [`is_ser_tuple`].
/// Thanks to this information we can expand tuple data into multiple arguments
/// while serializing vectors as a single value.
struct IsTupleSerde;
#[derive(Debug)]
struct IsTupleSerdeError(bool);
impl fmt::Display for IsTupleSerdeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IsTupleSerializerError: {}", self.0)
    }
}
impl std::error::Error for IsTupleSerdeError {}
impl serde::ser::Error for IsTupleSerdeError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        IsTupleSerdeError(false)
    }
}
impl serde::de::Error for IsTupleSerdeError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        IsTupleSerdeError(false)
    }
}

impl<'de> serde::Deserializer<'de> for IsTupleSerde {
    type Error = IsTupleSerdeError;

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str
        string unit unit_struct seq  map
        struct enum identifier ignored_any bytes byte_buf option
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(IsTupleSerdeError(false))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(IsTupleSerdeError(true))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(IsTupleSerdeError(true))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(IsTupleSerdeError(true))
    }
}

impl serde::Serializer for IsTupleSerde {
    type Ok = bool;
    type Error = IsTupleSerdeError;
    type SerializeSeq = Impossible<bool, IsTupleSerdeError>;
    type SerializeTuple = Impossible<bool, IsTupleSerdeError>;
    type SerializeTupleStruct = Impossible<bool, IsTupleSerdeError>;
    type SerializeTupleVariant = Impossible<bool, IsTupleSerdeError>;
    type SerializeMap = Impossible<bool, IsTupleSerdeError>;
    type SerializeStruct = Impossible<bool, IsTupleSerdeError>;
    type SerializeStructVariant = Impossible<bool, IsTupleSerdeError>;

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
        Err(IsTupleSerdeError(false))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(IsTupleSerdeError(true))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(IsTupleSerdeError(true))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(IsTupleSerdeError(false))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(IsTupleSerdeError(false))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(IsTupleSerdeError(false))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(IsTupleSerdeError(false))
    }
}

/// Returns true if the value is a tuple-like type according to the serde model.
pub fn is_ser_tuple<T: ?Sized + serde::Serialize>(value: &T) -> bool {
    match value.serialize(IsTupleSerde) {
        Ok(v) | Err(IsTupleSerdeError(v)) => v,
    }
}

/// Returns true if the type is a tuple-like type according to the serde model.
pub fn is_de_tuple<'de, T: serde::Deserialize<'de>>() -> bool {
    match T::deserialize(IsTupleSerde) {
        Ok(_) => unreachable!(),
        Err(IsTupleSerdeError(v)) => v,
    }
}

#[doc(hidden)]
#[cfg(test)]
pub mod test {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn is_tuple() {
        assert!(is_ser_tuple(&(1, 2, 3)));
        assert!(is_de_tuple::<(usize, usize, usize)>());

        assert!(is_ser_tuple(&[1, 2, 3]));
        assert!(is_de_tuple::<[usize; 3]>());

        #[derive(Serialize, Deserialize)]
        struct TupleStruct<'a>(&'a str);
        assert!(is_ser_tuple(&TupleStruct("test")));
        assert!(is_de_tuple::<TupleStruct<'_>>());

        assert!(!is_ser_tuple(&vec![1, 2, 3]));
        assert!(!is_de_tuple::<Vec<usize>>());

        #[derive(Serialize, Deserialize)]
        struct UnitStruct;
        assert!(!is_ser_tuple(&UnitStruct));
        assert!(!is_de_tuple::<UnitStruct>());
    }

    /// A stub parser that always returns an error. Only used for testing.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct StubParser;

    /// A stub error that is used for testing.
    #[derive(Serialize, Deserialize)]
    pub struct StubError;

    impl std::fmt::Debug for StubError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("StubError")
        }
    }
    impl std::fmt::Display for StubError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("StubError")
        }
    }
    impl std::error::Error for StubError {}

    fn stub_err() -> ParserError {
        ParserError {
            inner: Box::new(StubError),
        }
    }
    /// === impl StubParser ===
    impl Parse for StubParser {
        fn encode(self, _: Packet) -> Value {
            Value::Bytes(Bytes::new())
        }

        fn decode_str(self, _: &ParserState, _: Str) -> Result<Packet, ParseError> {
            Err(ParseError::ParserError(stub_err()))
        }

        fn decode_bin(self, _: &ParserState, _: bytes::Bytes) -> Result<Packet, ParseError> {
            Err(ParseError::ParserError(stub_err()))
        }

        fn encode_value<T: ?Sized + serde::Serialize>(
            self,
            _: &T,
            _: Option<&str>,
        ) -> Result<Value, ParserError> {
            Err(stub_err())
        }

        fn decode_value<'de, T: serde::Deserialize<'de>>(
            self,
            _: &'de mut Value,
            _: bool,
        ) -> Result<T, ParserError> {
            Err(stub_err())
        }

        fn decode_default<'de, T: serde::Deserialize<'de>>(
            self,
            _: Option<&'de Value>,
        ) -> Result<T, ParserError> {
            Err(stub_err())
        }

        fn encode_default<T: ?Sized + serde::Serialize>(self, _: &T) -> Result<Value, ParserError> {
            Err(stub_err())
        }

        fn read_event(self, _: &Value) -> Result<&str, ParserError> {
            Ok("")
        }
    }
}
