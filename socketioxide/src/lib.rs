//! Socket.IO server implementation as a (tower layer)[https://docs.rs/tower/latest/tower/] in Rust
//! It integrates with any framework that based on tower/hyper, such as:
//! * [axum](https://docs.rs/axum/latest/axum/)
//! * [warp](https://docs.rs/warp/latest/warp/)
//! * [hyper](https://docs.rs/hyper/latest/hyper/)
//! 
//! ## Usage
//! 
//! ```rust
//! use axum::routing::get;
//! use axum::Server;
//! use serde_json::Value;
//! use socketioxide::{Namespace, SocketIoLayer};
//! use tracing::info;
//! use tracing_subscriber::FmtSubscriber;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let subscriber = FmtSubscriber::builder().finish();
//!     tracing::subscriber::set_global_default(subscriber)?;
//! 
//!     info!("Starting server");
//! 
//!     let ns = Namespace::builder()
//!         .add("/", |socket| async move {
//!             info!("Socket.IO connected: {:?} {:?}", socket.ns(), socket.sid);
//!             let data: Value = socket.handshake.data().unwrap();
//!             socket.emit("auth", data).ok();
//! 
//!             socket.on("message", |socket, data: Value, bin, _| async move {
//!                 info!("Received event: {:?} {:?}", data, bin);
//!                 socket.bin(bin).emit("message-back", data).ok();
//!             });
//! 
//!             socket.on("message-with-ack", |_, data: Value, bin, ack| async move {
//!                 info!("Received event: {:?} {:?}", data, bin);
//!                 ack.bin(bin).send(data).ok();
//!             });
//!         })
//!         .add("/custom", |socket| async move {
//!             info!("Socket.IO connected on: {:?} {:?}", socket.ns(), socket.sid);
//!             let data: Value = socket.handshake.data().unwrap();
//!             socket.emit("auth", data).ok();
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
