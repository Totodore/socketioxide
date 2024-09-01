use bytes::Bytes;
use engineioxide::Str;
use serde::{de::DeserializeOwned, Serialize};

use crate::{packet::Packet, SocketIoValue};

/// All socket.io parser should implement this trait
pub trait Parse: Default {
    type Error: std::error::Error;
    /// Convert a packet into multiple payloads to be sent
    fn encode(&self, packet: Packet) -> SocketIoValue;

    /// Parse a given input string. If the payload needs more adjacent binary packet,
    /// the partial packet will be kept and a [`Error::NeedsMoreBinaryData`] will be returned
    fn decode_str(&self, data: Str) -> Result<Packet, ParseError<Self::Error>>;

    /// Parse a given input binary.
    fn decode_bin(&self, bin: Bytes) -> Result<Packet, ParseError<Self::Error>>;

    /// Convert any serializable data to a generic [`Bytes`]
    fn encode_value<T: Serialize>(
        &self,
        data: &T,
        event: Option<&str>,
    ) -> Result<SocketIoValue, Self::Error>;

    /// Convert any generic [`Bytes`] to deserializable data.
    ///
    /// The parser will be determined from the value given to deserialize.
    fn decode_value<T: DeserializeOwned>(
        &self,
        value: SocketIoValue,
        with_event: bool,
    ) -> Result<T, Self::Error>;
}

/// Errors when parsing/serializing socket.io packets
#[derive(thiserror::Error, Debug)]
pub enum ParseError<E: std::error::Error> {
    /// Invalid packet type
    #[error("invalid packet type")]
    InvalidPacketType,

    /// Invalid event name
    #[error("invalid event name")]
    InvalidEventName,

    /// Invalid event name
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
    /// e.g the common parser needs adjacent binary packets and therefore will returns [`NeedsMoreBinaryData`] n times for n adjacent binary packet expected.
    /// In this case the user should call again the parser with the next binary payload.
    #[error("needs more binary data before deserialization")]
    NeedsMoreBinaryData,

    #[error("parser error: {0:?}")]
    ParserError(#[from] E),
}
