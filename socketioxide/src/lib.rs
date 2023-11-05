//! Socket.IO server implementation as a [tower layer](https://docs.rs/tower/latest/tower/) in Rust.
//!
//! It integrates with any framework that based on tower/hyper, such as:
//! * [axum](https://docs.rs/axum/latest/axum/)
//! * [warp](https://docs.rs/warp/latest/warp/)
//! * [hyper](https://docs.rs/hyper/latest/hyper/)
//!
//! ## Usage with axum
//!
//! ```no_run
//! use axum::routing::get;
//! use axum::Server;
//! use serde::{Serialize, Deserialize};
//! use socketioxide::SocketIo;
//! use serde_json::Value;
//!
//! #[derive(Debug, Serialize, Deserialize)]
//! struct MyData {
//!   pub name: String,
//!   pub age: u8,
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!
//!     println!("Starting server");
//!
//!     let (layer, io) = SocketIo::new_layer();
//!
//!     io.ns("/", |socket: Arc<Socket>, auth: Value| async move {
//!         println!("Socket connected on / namespace with id: {}", socket.id);
//!
//!         // Add a callback triggered when the socket receive an 'abc' event
//!         // The json data will be deserialized to MyData
//!         socket.on("abc", |socket, data: MyData, bin, _| async move {
//!             println!("Received abc event: {:?} {:?}", data, bin);
//!             socket.bin(bin).emit("abc", data).ok();
//!         });
//!
//!         // Add a callback triggered when the socket receive an 'acb' event
//!         // Ackknowledge the message with the ack callback
//!         socket.on("acb", |_, data: Value, bin, ack| async move {
//!             println!("Received acb event: {:?} {:?}", data, bin);
//!             ack.bin(bin).send(data).ok();
//!         });
//!         // Add a callback triggered when the socket disconnect
//!         // The reason of the disconnection will be passed to the callback
//!         socket.on_disconnect(|socket, reason| async move {
//!             println!("Socket.IO disconnected: {} {}", socket.id, reason);
//!         });
//!     });
//!     
//!     io.ns("/custom", |socket: Arc<Socket>, auth: Value| async move {
//!         println!("Socket connected on /custom namespace with id: {}", socket.id);
//!     });
//!
//!     let app = axum::Router::new()
//!         .route("/", get(|| async { "Hello, World!" }))
//!         .layer(layer);
//!
//!     Server::bind(&"0.0.0.0:3000".parse().unwrap())
//!         .serve(app.into_make_service())
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

#[cfg(not(any(feature = "v4", feature = "v5")))]
compile_error!("At least one protocol version must be enabled");

pub mod adapter;

#[cfg(feature = "extensions")]
pub mod extensions;
#[cfg(feature = "hyper-v1")]
pub mod hyper_v1;
pub mod layer;
pub mod service;

#[cfg(feature = "test-utils")]
pub use packet::*;

pub use engineioxide::config::TransportType;
pub use errors::{AckError, AckSenderError, BroadcastError, Error as SocketError, SendError};
pub use handler::{AckResponse, AckSender};
pub use io::{SocketIo, SocketIoBuilder, SocketIoConfig};
pub use socket::{DisconnectReason, Socket};

mod client;
mod errors;
mod handler;
mod io;
mod ns;
mod operators;
mod packet;
mod socket;

/// Socket.IO protocol version
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    V4 = 4,
    V5 = 5,
}

impl From<ProtocolVersion> for engineioxide::ProtocolVersion {
    fn from(value: ProtocolVersion) -> Self {
        match value {
            ProtocolVersion::V4 => Self::V3,
            ProtocolVersion::V5 => Self::V4,
        }
    }
}
impl From<engineioxide::ProtocolVersion> for ProtocolVersion {
    fn from(value: engineioxide::ProtocolVersion) -> Self {
        match value {
            engineioxide::ProtocolVersion::V3 => Self::V4,
            engineioxide::ProtocolVersion::V4 => Self::V5,
        }
    }
}
