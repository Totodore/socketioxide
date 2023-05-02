pub mod adapters;

pub use config::{SocketIoConfig, SocketIoConfigBuilder};
pub use errors::{Error as SocketError, AckError};
pub use layer::SocketIoLayer;
pub use ns::Namespace;
pub use socket::{Ack, Socket};

mod client;
mod config;
mod errors;
mod handshake;
mod layer;
mod ns;
mod operators;
mod packet;
mod socket;
