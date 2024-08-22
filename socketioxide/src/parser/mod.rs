//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]
use bytes::Bytes;

mod common;
mod msgpack;
pub mod value;

pub use common::CommonParser;
pub use msgpack::MsgPackParser;
pub use value::Value;

use engineioxide::Str;
use serde::{de::DeserializeOwned, Serialize};
use value::ParseError;

use crate::packet::Packet;

/// Represent a socket.io payload that can be sent over an engine.io connection
pub enum TransportPayload {
    /// A string payload that will be sent as a string engine.io packet
    Str(engineioxide::Str),
    /// A binary payload that will be sent as a binary engine.io packet
    Bytes(bytes::Bytes),
}
impl TransportPayload {
    /// If the payload is a [`TransportPayload::Str`] or returns it
    /// or None otherwise.
    pub fn into_str(self) -> Option<engineioxide::Str> {
        match self {
            TransportPayload::Str(str) => Some(str),
            TransportPayload::Bytes(_) => None,
        }
    }

    /// If the payload is a [`TransportPayload::Bytes`] or returns it
    /// or None otherwise.
    pub fn into_bytes(self) -> Option<bytes::Bytes> {
        match self {
            TransportPayload::Str(_) => None,
            TransportPayload::Bytes(bytes) => Some(bytes),
        }
    }
}

/// All socket.io parser should implement this trait
pub trait Parse: Default {
    /// Convert a packet into multiple payloads to be sent
    fn encode(&self, packet: Packet<'_>) -> (TransportPayload, Vec<Bytes>);

    /// Parse a given input string. If the payload needs more adjacent binary packet,
    /// the partial packet will be kept and a [`Error::NeedsMoreBinaryData`] will be returned
    fn decode_str(&self, data: Str) -> Result<Packet<'static>, Error>;

    /// Parse a given input binary.
    fn decode_bin(&self, bin: Bytes) -> Result<Packet<'static>, Error>;

    /// Convert any serializable data to a generic [`Value`]
    fn to_value<T: Serialize>(&self, data: T) -> Result<Value, ParseError>;

    /// Convert any generic [`Value`] to deserializable data.
    ///
    /// The parser will be determined from the value given to deserialize.
    fn from_value<T: DeserializeOwned>(&self, value: &Value) -> Result<T, ParseError> {
        value::from_value(value)
    }
}

/// All the parser available.
/// It also implements the [`Parse`] trait and therefore the
/// parser implementation is done over enum dispatch.
#[non_exhaustive]
#[derive(Debug)]
pub enum Parser {
    /// The default parser
    Common(CommonParser),
    /// The MsgPack parser
    MsgPack(MsgPackParser),
}
impl Default for Parser {
    fn default() -> Self {
        Parser::Common(CommonParser::default())
    }
}

/// Recreate a new parser of the same type.
impl Clone for Parser {
    fn clone(&self) -> Self {
        match self {
            Parser::Common(_) => Parser::Common(CommonParser::default()),
            Parser::MsgPack(_) => Parser::MsgPack(MsgPackParser::default()),
        }
    }
}

impl Parse for Parser {
    fn encode(&self, packet: Packet<'_>) -> (TransportPayload, Vec<Bytes>) {
        match self {
            Parser::Common(p) => p.encode(packet),
            Parser::MsgPack(p) => p.encode(packet),
        }
    }

    fn decode_bin(&self, bin: Bytes) -> Result<Packet<'static>, Error> {
        let packet = match self {
            Parser::Common(p) => p.decode_bin(bin),
            Parser::MsgPack(p) => p.decode_bin(bin),
        };
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "bin payload decoded:");
        packet
    }
    fn decode_str(&self, data: Str) -> Result<Packet<'static>, Error> {
        let packet = match self {
            Parser::Common(p) => p.decode_str(data),
            Parser::MsgPack(p) => p.decode_str(data),
        };
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "str payload decoded:");
        packet
    }

    fn to_value<T: Serialize>(&self, data: T) -> Result<Value, ParseError> {
        match self {
            Parser::Common(p) => p.to_value(data),
            Parser::MsgPack(p) => p.to_value(data),
        }
    }
}

/// Errors when parsing/serializing socket.io packets
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Invalid packet type
    #[error("invalid packet type")]
    InvalidPacketType,

    /// Invalid event name
    #[error("invalid event name")]
    InvalidEventName,

    /// Invalid namespace
    #[error("invalid namespace")]
    InvalidNamespace,

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
    /// e.g the common parser needs adjacent binary packets and therefore will returns [`NeedsMoreBinaryData`] n times for n adjacent binary packet expected.
    /// In this case the user should call again the parser with the next binary payload.
    #[error("needs more binary data before deserialization")]
    NeedsMoreBinaryData,

    /// Error serializing json packet
    #[error("error serializing/deserializing packet: {0:?}")]
    ParseError(#[from] value::ParseError),
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::ParseError(ParseError::Json(error))
    }
}
impl From<rmp_serde::decode::Error> for Error {
    fn from(error: rmp_serde::decode::Error) -> Self {
        Error::ParseError(ParseError::MsgPackDecode(error))
    }
}
