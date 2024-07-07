//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]
use bytes::Bytes;

mod common;
mod msgpack;

pub use common::CommonParser;
use engineioxide::Str;

use crate::packet::Packet;
pub enum TransportPayload {
    Str(engineioxide::Str),
    Bytes(bytes::Bytes),
}
pub trait Parse: Default {
    /// Convert a packet into multiple payloads to be sent
    fn serialize<'a>(&self, packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>);

    /// Parse a given input string. If the payload needs more adjacent binary packet,
    /// the partial packet will be kept and a [`Error::NeedsMoreBinaryData`] will be returned
    fn parse_str(&self, data: Str) -> Result<Packet<'static>, Error>;

    /// Parse a given input binary.
    fn parse_bin(&self, bin: Bytes) -> Result<Packet<'static>, Error>;
}

#[derive(Debug)]
pub enum Parser {
    Common(CommonParser),
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
        }
    }
}

impl Parse for Parser {
    fn serialize<'a>(&self, packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>) {
        match self {
            Parser::Common(p) => p.serialize(packet),
        }
    }

    fn parse_bin(&self, bin: Bytes) -> Result<Packet<'static>, Error> {
        match self {
            Parser::Common(p) => p.parse_bin(bin),
        }
    }
    fn parse_str(&self, data: Str) -> Result<Packet<'static>, Error> {
        match self {
            Parser::Common(p) => p.parse_str(data),
        }
    }
}
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("invalid packet type")]
    InvalidPacketType,

    #[error("invalid event name")]
    InvalidEventName,

    #[error("invalid namespace")]
    InvalidNamespace,

    #[error(
        "received unexpected binary data. Make sure you are using the same parser on both ends."
    )]
    UnexpectedBinaryPacket,

    #[error(
        "received unexpected string data. Make sure you are using the same parser on both ends."
    )]
    UnexpectedStringPacket,

    #[error("needs more binary data before deserialization")]
    NeedsMoreBinaryData,

    #[error("error serializing json packet: {0:?}")]
    Serialize(#[from] serde_json::Error),
}
