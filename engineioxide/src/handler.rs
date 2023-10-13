use std::sync::Arc;

use async_trait::async_trait;

use crate::socket::{DisconnectReason, Socket};

/// An handler for engine.io events for each sockets.
#[async_trait]
pub trait EngineIoHandler: std::fmt::Debug + Send + Sync + 'static {
    /// Data associated with the socket.
    type Data: Default + Send + Sync + 'static;

    /// Called when a new socket is connected.
    fn on_connect(&self, socket: Arc<Socket<Self::Data>>);

    /// Called when a socket is disconnected.
    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason);

    /// Called when a message is received from the client.
    fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>);

    /// Called when a binary message is received from the client.
    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<Self::Data>>);
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

    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<Self::Data>>) {
        (**self).on_binary(data, socket)
    }
}
