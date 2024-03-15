//! ## An [`EngineIoHandler`] to get event calls for any engine.io socket
//! #### Example :
//! ```rust
//! # use bytes::Bytes;
//! # use engineioxide::service::EngineIoService;
//! # use engineioxide::handler::EngineIoHandler;
//! # use engineioxide::{Socket, DisconnectReason};
//! # use std::sync::{Arc, Mutex};
//! # use std::sync::atomic::{AtomicUsize, Ordering};
//! // Global state
//! #[derive(Debug, Default)]
//! struct MyHandler {
//!     user_cnt: AtomicUsize,
//! }
//!
//! // Socket state
//! #[derive(Debug, Default)]
//! struct SocketState {
//!     id: Mutex<String>,
//! }
//!
//! impl EngineIoHandler for MyHandler {
//!     type Data = SocketState;
//!
//!     fn on_connect(&self, socket: Arc<Socket<SocketState>>) {
//!         let cnt = self.user_cnt.fetch_add(1, Ordering::Relaxed) + 1;
//!         socket.emit(cnt.to_string()).ok();
//!     }
//!     fn on_disconnect(&self, socket: Arc<Socket<SocketState>>, reason: DisconnectReason) {
//!         let cnt = self.user_cnt.fetch_sub(1, Ordering::Relaxed) - 1;
//!         socket.emit(cnt.to_string()).ok();
//!     }
//!     fn on_message(&self, msg: String, socket: Arc<Socket<SocketState>>) {
//!         *socket.data.id.lock().unwrap() = msg; // bind a provided user id to a socket
//!     }
//!     fn on_binary(&self, data: Bytes, socket: Arc<Socket<SocketState>>) { }
//! }
//!
//! // Create an engine io service with the given handler
//! let svc = EngineIoService::new(MyHandler::default());
//! ```
use std::sync::Arc;

use bytes::Bytes;

use crate::socket::{DisconnectReason, Socket};

/// The [`EngineIoHandler`] trait can be implemented on any struct to handle socket events
///
/// A `Data` associated type can be specified to attach a custom state to the sockets
pub trait EngineIoHandler: std::fmt::Debug + Send + Sync + 'static {
    /// Data associated with the socket.
    type Data: Default + Send + Sync + 'static;

    /// Called when a new socket is connected.
    fn on_connect(&self, socket: Arc<Socket<Self::Data>>);

    /// Called when a socket is disconnected with a [`DisconnectReason`]
    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason);

    /// Called when a message is received from the client.
    fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>);

    /// Called when a binary message is received from the client.
    fn on_binary(&self, data: Bytes, socket: Arc<Socket<Self::Data>>);
}

impl<T: EngineIoHandler> EngineIoHandler for Arc<T> {
    type Data = T::Data;

    fn on_connect(&self, socket: Arc<Socket<Self::Data>>) {
        (**self).on_connect(socket)
    }

    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
        (**self).on_disconnect(socket, reason)
    }

    fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>) {
        (**self).on_message(msg, socket)
    }

    fn on_binary(&self, data: Bytes, socket: Arc<Socket<Self::Data>>) {
        (**self).on_binary(data, socket)
    }
}
