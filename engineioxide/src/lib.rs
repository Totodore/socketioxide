pub use async_trait::async_trait;

pub use server::{
    EngineIoLayer, EngineIoService, MakeEngineIoService, NotFoundService, ProtocolVersion,
};
/// A Packet type to use when sending data to the client
pub use socket::{DisconnectReason, Socket, SocketReq};

#[cfg(not(any(feature = "v3", feature = "v4")))]
compile_error!("At least one protocol version must be enabled");

pub mod config;
pub mod errors;
pub mod handler;
pub mod sid;
pub mod socket;

mod engine;
mod packet;
mod peekable;
mod server;
mod transport;
