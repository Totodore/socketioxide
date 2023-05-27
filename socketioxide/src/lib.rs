pub mod adapter;

pub use config::{SocketIoConfig, SocketIoConfigBuilder};
pub use errors::{AckError, Error as SocketError};
pub use layer::SocketIoLayer;
pub use ns::Namespace;
pub use socket::Socket;

mod client;
mod config;
mod errors;
mod handler;
mod handshake;
mod layer;
mod ns;
mod operators;
mod packet;
mod socket;
