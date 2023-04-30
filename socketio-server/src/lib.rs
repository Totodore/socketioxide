pub mod adapter;

pub use config::{SocketIoConfig, SocketIoConfigBuilder};
pub use errors::Error as SocketError;
pub use layer::SocketIoLayer;
pub use ns::Namespace;
pub use socket::{Ack, Socket};

mod client;
mod config;
mod errors;
mod handshake;
mod layer;
mod ns;
mod operator;
mod packet;
mod socket;
