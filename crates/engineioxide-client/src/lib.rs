#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/Totodore/socketioxide/refs/heads/main/.github/logo_dark.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/Totodore/socketioxide/refs/heads/main/.github/logo_dark.ico"
)]
//! Engine.IO client library for Rust.

mod client;
mod config;
mod errors;
mod transport;

pub mod flavors;

pub use crate::client::Client;
pub use config::{EngineIoClientConfig, EngineIoClientConfigBuilder};
pub use engineioxide_core::{Packet, Sid, Str, TransportType};
pub use errors::{ClientError, ConnectError};
pub use transport::{PollingError, ProtocolError, UpgradeError, WsError};

use bytes::Bytes;

/// An event streamed from the Engine.IO client.
#[derive(Debug, PartialEq)]
pub enum EioEvent {
    /// Got a connection to the server.
    Connect(Sid),
    /// Got disconnected from the server.
    Disconnect,
    /// Got a message from the server.
    Message(Str),
    /// Got binary data from the server.
    Binary(Bytes),
    /// The client transport got upgraded to this new transport type.
    Upgrade(TransportType),
}

impl From<EioEvent> for Option<Packet> {
    fn from(value: EioEvent) -> Option<Packet> {
        match value {
            EioEvent::Message(msg) => Some(Packet::Message(msg)),
            EioEvent::Binary(bin) => Some(Packet::Binary(bin)),
            _ => None,
        }
    }
}
