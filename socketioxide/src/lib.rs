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
//! use socketioxide::{Namespace, SocketIoLayer};
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
//!     let ns = Namespace::builder()
//!         .add("/", |socket| async move {
//!             println!("Socket connected on / namespace with id: {}", socket.sid);
//!
//!             // Add a callback triggered when the socket receive an 'abc' event
//!             // The json data will be deserialized to MyData
//!             socket.on("abc", |socket, data: MyData, bin, _| async move {
//!                 println!("Received abc event: {:?} {:?}", data, bin);
//!                 socket.bin(bin).emit("abc", data).ok();
//!             });
//!
//!             // Add a callback triggered when the socket receive an 'acb' event
//!             // Ackknowledge the message with the ack callback
//!             socket.on("acb", |_, data: Value, bin, ack| async move {
//!                 println!("Received acb event: {:?} {:?}", data, bin);
//!                 ack.bin(bin).send(data).ok();
//!             });
//!         })
//!         .add("/custom", |socket| async move {
//!             println!("Socket connected on /custom namespace with id: {}", socket.sid);
//!         })
//!         .build();
//!
//!     let app = axum::Router::new()
//!         .route("/", get(|| async { "Hello, World!" }))
//!         .layer(SocketIoLayer::new(ns));
//!
//!     Server::bind(&"0.0.0.0:3000".parse().unwrap())
//!         .serve(app.into_make_service())
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

pub mod adapter;
pub mod extensions;

pub use config::{SocketIoConfig, SocketIoConfigBuilder};
pub use errors::{
    AckError, AckSenderError, BroadcastError, Error as SocketError, SendError, TransportError,
};
pub use layer::SocketIoLayer;
pub use ns::Namespace;
pub use service::SocketIoService;
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
mod service;
mod socket;
