use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use engineioxide_core::Sid;
use http::request::Parts;

use crate::{
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::{ProtocolVersion, TransportType},
    socket::{DisconnectReason, Socket},
};

type SocketMap<T> = RwLock<HashMap<Sid, Arc<T>>>;

/// The [`EngineIo`] struct holds the state of the engine.io server as well as utility methods to manage the state
pub struct EngineIo<H: EngineIoHandler> {
    /// A map of all the sockets connected to the server
    sockets: SocketMap<Socket<H::Data>>,

    /// The handler for the engine.io server that will be called when events are received
    pub handler: Arc<H>,

    /// The config for the engine.io server
    pub config: EngineIoConfig,
}

impl<H: EngineIoHandler> EngineIo<H> {
    /// Create a new Engine.IO server with a [`EngineIoHandler`] and a [`EngineIoConfig`]
    pub fn new(handler: Arc<H>, config: EngineIoConfig) -> Self {
        Self {
            sockets: RwLock::new(HashMap::new()),
            config,
            handler,
        }
    }
}

impl<H: EngineIoHandler> EngineIo<H> {
    /// Create a new engine.io session and a new socket and add it to the socket map
    pub(crate) fn create_session(
        self: &Arc<Self>,
        protocol: ProtocolVersion,
        transport: TransportType,
        req: Parts,
        #[cfg(feature = "v3")] supports_binary: bool,
    ) -> Arc<Socket<H::Data>> {
        let engine = self.clone();
        let close_fn = Box::new(move |sid, reason| engine.close_session(sid, reason));

        let socket = Socket::new(
            protocol,
            transport,
            &self.config,
            req,
            close_fn,
            #[cfg(feature = "v3")]
            supports_binary,
        );
        let socket = Arc::new(socket);
        self.sockets
            .write()
            .unwrap()
            .insert(socket.id, socket.clone());
        self.handler.clone().on_connect(socket.clone());
        socket
    }

    /// Get a socket by its sid
    /// Clones the socket ref to avoid holding the lock
    pub fn get_socket(&self, sid: Sid) -> Option<Arc<Socket<H::Data>>> {
        self.sockets.read().unwrap().get(&sid).cloned()
    }

    /// Close an engine.io session by removing the socket from the socket map and closing the socket.
    ///
    /// It should be the only way to close a session and to remove a socket from the socket map.
    #[cfg_attr(feature = "tracing", tracing::instrument(skip(self)))]
    pub fn close_session(&self, sid: Sid, reason: DisconnectReason) {
        let Some(socket) = self.sockets.write().unwrap().remove(&sid) else {
            #[cfg(feature = "tracing")]
            tracing::debug!(
                ?sid,
                socket_len = self.sockets.read().unwrap().len(),
                "socket not found"
            );
            return;
        };

        // Try to close the internal channel if it is available
        // E.g. with polling transport the channel is not always locked so it is necessary to close it here
        socket.internal_rx.try_lock().map(|mut rx| rx.close()).ok();
        socket.abort_heartbeat();
        self.handler.on_disconnect(socket, reason);

        #[cfg(feature = "tracing")]
        tracing::debug!(
            rem_sockets = self.sockets.read().unwrap().len(),
            "remaining sockets",
        );
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use engineioxide_core::Str;
    use http::Request;

    use super::*;

    #[derive(Debug, Clone)]
    struct MockHandler;

    impl EngineIoHandler for MockHandler {
        type Data = ();

        fn on_connect(self: Arc<Self>, socket: Arc<Socket<Self::Data>>) {
            println!("socket connect {}", socket.id);
        }

        fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {
            println!("socket disconnect {} {:?}", socket.id, reason);
        }

        fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<Self::Data>>) {
            println!("Ping pong message {msg:?}");
            socket.emit(msg).ok();
        }

        fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<Self::Data>>) {
            println!("Ping pong binary message {data:?}");
            socket.emit_binary(data).ok();
        }
    }

    fn create_engine() -> Arc<EngineIo<MockHandler>> {
        let config = EngineIoConfig::default();
        Arc::new(EngineIo::new(Arc::new(MockHandler), config))
    }

    #[tokio::test]
    async fn create_session() {
        let engine = create_engine();
        let socket = engine.create_session(
            ProtocolVersion::V4,
            TransportType::Polling,
            Request::<()>::default().into_parts().0,
            #[cfg(feature = "v3")]
            true,
        );
        assert_eq!(engine.sockets.read().unwrap().len(), 1);
        assert_eq!(socket.protocol, ProtocolVersion::V4);
        assert!(socket.is_http());
    }

    #[tokio::test]
    async fn close_session() {
        let engine = create_engine();
        let socket = engine.create_session(
            ProtocolVersion::V4,
            TransportType::Polling,
            Request::<()>::default().into_parts().0,
            #[cfg(feature = "v3")]
            true,
        );
        assert_eq!(engine.sockets.read().unwrap().len(), 1);
        engine.close_session(socket.id, DisconnectReason::TransportClose);
        assert_eq!(engine.sockets.read().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn get_socket() {
        let engine = create_engine();
        let socket = engine.create_session(
            ProtocolVersion::V4,
            TransportType::Polling,
            Request::<()>::default().into_parts().0,
            #[cfg(feature = "v3")]
            true,
        );
        assert_eq!(engine.sockets.read().unwrap().len(), 1);
        let socket = engine.get_socket(socket.id).unwrap();
        assert_eq!(socket.protocol, ProtocolVersion::V4);
        assert!(socket.is_http());
    }
}
