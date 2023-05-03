pub mod adapter;

pub use config::{SocketIoConfig, SocketIoConfigBuilder};
pub use errors::{Error as SocketError, AckError};
pub use layer::SocketIoLayer;
pub use ns::Namespace;
pub use socket::{Ack, Socket, AckResponse};
pub use packet::Packet;

mod client;
mod config;
mod errors;
mod handshake;
mod layer;
mod ns;
mod operators;
mod packet;
mod socket;
// local_adapter and remote_adapter should not be enabled at the same time
#[cfg(all(feature = "local_adapter", feature = "remote_adapter"))]
compile_error!("local_adapter and remote_adapter feature flags should not be enabled at the same time");