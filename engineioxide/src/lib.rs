#![doc = include_str!("../Readme.md")]

pub use service::{ProtocolVersion, TransportType};
pub use socket::{DisconnectReason, Socket};

pub mod config;
pub mod handler;
pub mod layer;
pub mod service;
pub mod sid;
pub mod socket;

mod body;
mod engine;
mod packet;
mod peekable;
mod transport;
mod errors;