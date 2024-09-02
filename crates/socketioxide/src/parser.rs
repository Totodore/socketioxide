//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]
use bytes::Bytes;

use socketioxide_core::parser::{Parse, ParseError, SocketIoValue};

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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    Common(<CommonParser as Parse>::Error),
    MsgPack(<MsgPackParser as Parse>::Error),
}

impl Parse for Parser {
    type Error = Error;

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

    fn encode_value<T: Serialize>(
        &self,
        data: &T,
        event: Option<&str>,
    ) -> Result<SocketIoValue, ParseError> {
        match self {
            Parser::Common(p) => p.encode_value(data, event),
            Parser::MsgPack(p) => p.encode_value(data, event),
        }
    }

    fn decode_value<T: DeserializeOwned>(
        &self,
        value: &SocketIoValue,
        with_event: bool,
    ) -> Result<T, Self::Error> {
        match self {
            Parser::Common(p) => p.decode_value(value, with_event),
            Parser::MsgPack(p) => p.decode_value(value, with_event),
        }
    }
}
