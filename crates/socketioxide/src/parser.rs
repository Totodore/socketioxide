//! Contains all the parser implementations for the socket.io protocol.
//!
//! The default parser is the [`CommonParser`]

use bytes::Bytes;

use socketioxide_core::{
    Str, Value,
    packet::Packet,
    parser::{Parse, ParserState},
};

use serde::{Deserialize, Serialize};
use socketioxide_parser_common::CommonParser;

#[cfg(feature = "msgpack")]
use socketioxide_parser_msgpack::MsgPackParser;

pub(crate) use socketioxide_core::parser::ParseError;
pub use socketioxide_core::parser::ParserError;

/// All the parser available.
/// It also implements the [`Parse`] trait and therefore the
/// parser implementation is done over enum delegation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub(crate) enum Parser {
    /// The default parser
    Common(CommonParser),
    /// The MsgPack parser
    #[cfg(feature = "msgpack")]
    MsgPack(MsgPackParser),
}

impl Default for Parser {
    fn default() -> Self {
        Parser::Common(CommonParser)
    }
}

impl Parse for Parser {
    fn encode(self, packet: Packet) -> Value {
        let value = match self {
            Parser::Common(p) => p.encode(packet),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.encode(packet),
        };
        #[cfg(feature = "tracing")]
        match &value {
            Value::Str(value, bins) => tracing::trace!(?value, ?bins, "packet encoded:"),
            Value::Bytes(value) => tracing::trace!("packet encoded: {:X}", value),
        }

        value
    }

    fn decode_bin(self, state: &ParserState, bin: Bytes) -> Result<Packet, ParseError> {
        #[cfg(feature = "tracing")]
        tracing::trace!(?state, "decoding bin payload: {:X}", bin);

        let packet = match self {
            Parser::Common(p) => p.decode_bin(state, bin),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.decode_bin(state, bin),
        }?;

        #[cfg(feature = "tracing")]
        tracing::trace!(?packet, "bin payload decoded:");
        Ok(packet)
    }
    fn decode_str(self, state: &ParserState, data: Str) -> Result<Packet, ParseError> {
        #[cfg(feature = "tracing")]
        tracing::trace!(?data, ?state, "decoding str payload:");

        let packet = match self {
            Parser::Common(p) => p.decode_str(state, data),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.decode_str(state, data),
        }?;

        #[cfg(feature = "tracing")]
        tracing::trace!(?packet, "str payload decoded:");
        Ok(packet)
    }

    fn encode_value<T: ?Sized + Serialize>(
        self,
        data: &T,
        event: Option<&str>,
    ) -> Result<Value, ParserError> {
        let value = match self {
            Parser::Common(p) => p.encode_value(data, event),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.encode_value(data, event),
        };
        #[cfg(feature = "tracing")]
        tracing::trace!(?value, "value encoded:");
        value
    }

    fn decode_value<'de, T: Deserialize<'de>>(
        self,
        value: &'de mut Value,
        with_event: bool,
    ) -> Result<T, ParserError> {
        #[cfg(feature = "tracing")]
        tracing::trace!(?value, "decoding value:");
        match self {
            Parser::Common(p) => p.decode_value(value, with_event),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.decode_value(value, with_event),
        }
    }

    fn decode_default<'de, T: Deserialize<'de>>(
        self,
        value: Option<&'de Value>,
    ) -> Result<T, ParserError> {
        match self {
            Parser::Common(p) => p.decode_default(value),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.decode_default(value),
        }
    }

    fn encode_default<T: ?Sized + Serialize>(self, data: &T) -> Result<Value, ParserError> {
        match self {
            Parser::Common(p) => p.encode_default(data),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.encode_default(data),
        }
    }

    fn read_event(self, value: &Value) -> Result<&str, ParserError> {
        match self {
            Parser::Common(p) => p.read_event(value),
            #[cfg(feature = "msgpack")]
            Parser::MsgPack(p) => p.read_event(value),
        }
    }
}
