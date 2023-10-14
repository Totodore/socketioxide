use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    config::EngineIoConfig,
    handler::EngineIoHandler,
    sid_generator::generate_sid,
    socket::{ConnectionType, DisconnectReason, Socket, SocketReq},
};
use crate::{service::ProtocolVersion, sid_generator::Sid};
use tracing::debug;

type SocketMap<T> = RwLock<HashMap<Sid, Arc<T>>>;

/// Abstract engine implementation for Engine.IO server for http polling and websocket
/// It handle all the connection logic and dispatch the packets to the socket
pub struct EngineIo<H: EngineIoHandler> {
    sockets: SocketMap<Socket<H::Data>>,
    pub handler: H,
    pub config: EngineIoConfig,
}

impl<H: EngineIoHandler> EngineIo<H> {
    /// Create a new Engine.IO server with a handler and a config
    pub fn new(handler: H, config: EngineIoConfig) -> Self {
        Self {
            sockets: RwLock::new(HashMap::new()),
            config,
            handler,
        }
    }
}

impl<H: EngineIoHandler> EngineIo<H> {
    pub(crate) fn create_session(
        self: &Arc<Self>,
        protocol: ProtocolVersion,
        conn: ConnectionType,
        req: SocketReq,
        #[cfg(feature = "v3")] supports_binary: bool,
    ) -> Arc<Socket<H::Data>> {
        let engine = self.clone();
        let close_fn = Box::new(move |sid, reason| engine.close_session(sid, reason));

        let sid = generate_sid();

        let socket = Socket::new(
            sid,
            protocol,
            conn,
            &self.config,
            req,
            close_fn,
            #[cfg(feature = "v3")]
            supports_binary,
        );
        let socket = Arc::new(socket);
        {
            self.sockets.write().unwrap().insert(sid, socket.clone());
        }
        socket
    }

    /// Get a socket by its sid
    /// Clones the socket ref to avoid holding the lock
    pub fn get_socket(&self, sid: Sid) -> Option<Arc<Socket<H::Data>>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }

    /// Close an engine.io session by removing the socket from the socket map and closing the socket
    /// It should be the only way to close a session and to remove a socket from the socket map
    pub fn close_session(&self, sid: Sid, reason: DisconnectReason) {
        let socket = self.sockets.write().unwrap().remove(&sid);
        if let Some(socket) = socket {
            // Try to close the internal channel if it is available
            // For e.g with polling transport the channel is not always locked so it is necessary to close it here
            socket.internal_rx.try_lock().map(|mut rx| rx.close()).ok();
            socket.abort_heartbeat();
            self.handler.on_disconnect(socket, reason);
            debug!(
                "remaining sockets: {:?}",
                self.sockets.read().unwrap().len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    #[derive(Debug, Clone)]
    struct MockHandler;

    #[async_trait]
    impl EngineIoHandler for MockHandler {
        type Data = ();

        fn on_connect(&self, socket: Arc<Socket<Self::Data>>) {
            println!("socket connect {}", socket.id);
        }

        fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
            println!("socket disconnect {} {:?}", socket.id, reason);
        }

        fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>) {
            println!("Ping pong message {:?}", msg);
            socket.emit(msg).ok();
        }

        fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<Self::Data>>) {
            println!("Ping pong binary message {:?}", data);
            socket.emit_binary(data).ok();
        }
    }
}
