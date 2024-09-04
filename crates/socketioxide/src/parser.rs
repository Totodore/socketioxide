//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]

use bytes::Bytes;

use socketioxide_core::{
    parser::{Parse, ParseError},
    Value,
};

use engineioxide::Str;
use serde::{de::DeserializeOwned, Serialize};
use socketioxide_parser_common::CommonParser;
use socketioxide_parser_msgpack::MsgPackParser;

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
pub enum EncodeError {
    #[error("common parser: {0}")]
    Common(<CommonParser as Parse>::EncodeError),
    #[error("msgpack parser: {0}")]
    MsgPack(<MsgPackParser as Parse>::EncodeError),
}
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("common parser: {0}")]
    Common(#[from] <CommonParser as Parse>::DecodeError),
    #[error("msgpack parser: {0}")]
    MsgPack(#[from] <MsgPackParser as Parse>::DecodeError),
}

impl Parse for Parser {
    type EncodeError = EncodeError;
    type DecodeError = DecodeError;

    fn encode(&self, packet: Packet) -> Value {
        match self {
            Parser::Common(p) => p.encode(packet),
            Parser::MsgPack(p) => p.encode(packet),
        }
    }

    fn decode_bin(&self, bin: Bytes) -> Result<Packet, ParseError<DecodeError>> {
        let packet = match self {
            Parser::Common(p) => p.decode_bin(bin)?,
            Parser::MsgPack(p) => p.decode_bin(bin)?,
        }?;
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "bin payload decoded:");
        Ok(packet)
    }
    fn decode_str(&self, data: Str) -> Result<Packet, ParseError<DecodeError>> {
        let packet = match self {
            Parser::Common(p) => p.decode_str(data)?,
            Parser::MsgPack(p) => p.decode_str(data)?,
        };
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "str payload decoded:");
        Ok(packet)
    }

    fn encode_value<T: Serialize>(
        &self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, EncodeError> {
        match self {
            Parser::Common(p) => p.encode_value(data, event),
            Parser::MsgPack(p) => p.encode_value(data, event),
        }
    }

    fn decode_value<T: DeserializeOwned>(
        &self,
        value: Value,
        with_event: bool,
    ) -> Result<T, DecodeError> {
        match self {
            Parser::Common(p) => p.decode_value(value, with_event),
            Parser::MsgPack(p) => p.decode_value(value, with_event),
        }
    }
}
