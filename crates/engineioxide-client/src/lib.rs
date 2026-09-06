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

pub mod flavors;
pub mod transport;

pub use crate::client::Client;
pub use config::{EngineIoClientConfig, EngineIoClientConfigBuilder};
pub use engineioxide_core::{Packet, Sid, Str, TransportType};
pub use errors::{ClientError, ConnectError};

use bytes::Bytes;

#[derive(Debug, PartialEq)]
pub enum EioEvent {
    Connect(Sid),
    Disconnect,
    Message(Str),
    Binary(Bytes),
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
