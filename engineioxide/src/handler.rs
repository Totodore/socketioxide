use async_trait::async_trait;

use crate::socket::{Socket, DisconnectReason};

/// An handler for engine.io events for each sockets.
#[async_trait]
pub trait EngineIoHandler: std::fmt::Debug + Send + Sync + Clone + 'static {
    /// Data associated with the socket.
    type Data: Default + Send + Sync + std::fmt::Debug + 'static;

    /// Called when a new socket is connected.
    fn on_connect(&self, socket: &Socket<Self>);

    /// Called when a socket is disconnected.
    fn on_disconnect(&self, socket: &Socket<Self>, reason: DisconnectReason);

    /// Called when a message is received from the client.
    fn on_message(&self, msg: String, socket: &Socket<Self>);

    /// Called when a binary message is received from the client.
    fn on_binary(&self, data: Vec<u8>, socket: &Socket<Self>);
}
