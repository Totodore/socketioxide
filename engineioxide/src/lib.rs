pub use async_trait::async_trait;

/// A Packet type to use when sending data to the client
pub use packet::SendPacket;

#[cfg(not(any(feature = "v3", feature = "v4")))]
compile_error!("At least one protocol version must be enabled");

pub mod config;
pub mod engine;
pub mod errors;
pub mod handler;
pub mod layer;
pub mod service;
pub mod sid_generator;
pub mod socket;

mod body;
mod futures;
mod packet;
mod payload;
mod utils;
