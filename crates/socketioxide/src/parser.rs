//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]

use bytes::Bytes;

use socketioxide_core::{
    packet::Packet,
    parser::{Parse, ParserState},
    Value,
};

use engineioxide::Str;
use serde::{de::DeserializeOwned, Serialize};
use socketioxide_parser_common::CommonParser;
use socketioxide_parser_msgpack::MsgPackParser;

/// All the parser available.
/// It also implements the [`Parse`] trait and therefore the
/// parser implementation is done over enum delegation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
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
    Common(<CommonParser as Parse>::DecodeError),
    #[error("msgpack parser: {0}")]
    MsgPack(<MsgPackParser as Parse>::DecodeError),
}
pub type ParseError = socketioxide_core::parser::ParseError<DecodeError>;

impl Parse for Parser {
    type EncodeError = EncodeError;
    type DecodeError = DecodeError;

    fn encode(self, packet: Packet) -> Value {
        match self {
            Parser::Common(p) => p.encode(packet),
            Parser::MsgPack(p) => p.encode(packet),
        }
    }

    fn decode_bin(self, state: &ParserState, bin: Bytes) -> Result<Packet, ParseError> {
        let packet = match self {
            Parser::Common(p) => p
                .decode_bin(state, bin)
                .map_err(|e| e.wrap_err(DecodeError::Common)),
            Parser::MsgPack(p) => p
                .decode_bin(state, bin)
                .map_err(|e| e.wrap_err(DecodeError::MsgPack)),
        }?;
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "bin payload decoded:");
        Ok(packet)
    }
    fn decode_str(self, state: &ParserState, data: Str) -> Result<Packet, ParseError> {
        let packet = match self {
            Parser::Common(p) => p
                .decode_str(state, data)
                .map_err(|e| e.wrap_err(DecodeError::Common)),
            Parser::MsgPack(p) => p
                .decode_str(state, data)
                .map_err(|e| e.wrap_err(DecodeError::MsgPack)),
        }?;
        #[cfg(feature = "tracing")]
        tracing::debug!(?packet, "str payload decoded:");
        Ok(packet)
    }

    fn encode_value<T: Serialize>(
        self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, EncodeError> {
        match self {
            Parser::Common(p) => p.encode_value(data, event).map_err(EncodeError::Common),
            Parser::MsgPack(p) => p.encode_value(data, event).map_err(EncodeError::MsgPack),
        }
    }

    fn decode_value<T: DeserializeOwned>(
        self,
        value: &Value,
        with_event: bool,
    ) -> Result<T, DecodeError> {
        match self {
            Parser::Common(p) => p
                .decode_value(value, with_event)
                .map_err(DecodeError::Common),
            Parser::MsgPack(p) => p
                .decode_value(value, with_event)
                .map_err(DecodeError::MsgPack),
        }
    }

    fn value_none(self) -> Value {
        match self {
            Parser::Common(p) => p.value_none(),
            Parser::MsgPack(p) => p.value_none(),
        }
    }
}
