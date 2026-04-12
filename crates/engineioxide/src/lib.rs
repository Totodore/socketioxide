#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub use engineioxide_core::Str;
pub use service::{ProtocolVersion, TransportType};
pub use socket::{DisconnectReason, Socket};

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
pub use packet::*;

pub mod config;
pub mod handler;
pub mod layer;
pub mod service;
pub mod socket;

/// Socket id type and generator
pub mod sid {
    #[deprecated(since = "0.16.2", note = "Use engineioxide::socket::Sid instead")]
    pub use engineioxide_core::Sid;
}

mod body;
mod engine;
mod errors;
mod packet;
mod peekable;
mod transport;
