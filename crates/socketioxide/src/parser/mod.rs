//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]
use bytes::Bytes;

use socketioxide_core::parser::{Parse, SocketIoValue};

use engineioxide::Str;
use serde::{de::DeserializeOwned, Serialize};
use socketioxide_parser_common::CommonParser;
use socketioxide_parser_msgpack::MsgPackParser;
use value::ParseError;

use crate::packet::Packet;

/// All the parser available.
/// It also implements the [`Parse`] trait and therefore the
/// parser implementation is done over enum delegation.
#[non_exhaustive]
#[derive(Debug)]
pub enum Parser {
    /// The default parser
    Common(CommonParser),
    /// The MsgPack parser
    MsgPack(MsgPackParser),
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("json error: {0}")]
    Json(#[from] serde_json::error::Error),
    #[error("msgpack encode error: {0}")]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode error: {0}")]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
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
    type Error = ParseError;

    fn encode(&self, packet: Packet<'_>) -> (SocketIoValue, Vec<Bytes>) {
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

    fn encode_value<T: Serialize>(&self, data: &T) -> Result<SocketIoValue, ParseError> {
        match self {
            Parser::Common(p) => p.encode_value(data),
            Parser::MsgPack(p) => p.to_value(data),
        }
    }

    fn decode_value<T: DeserializeOwned>(&self, value: &SocketIoValue) -> Result<T, Self::Error> {
        match self {
            Parser::Common(p) => p.decode_value(value),
            Parser::MsgPack(p) => p.from_value(value),
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
