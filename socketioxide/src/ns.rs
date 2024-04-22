use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    adapter::Adapter,
    errors::{ConnectFail, Error},
    handler::{BoxedConnectHandler, ConnectHandler, MakeErasedHandler},
    packet::{Packet, PacketData},
    socket::Socket,
    SocketIoConfig,
};
use crate::{client::SocketData, errors::AdapterError};
use engineioxide::sid::Sid;

pub struct Namespace<A: Adapter> {
    pub path: Cow<'static, str>,
    pub(crate) adapter: A,
    handler: BoxedConnectHandler<A>,
    sockets: RwLock<HashMap<Sid, Arc<Socket<A>>>>,
}

impl<A: Adapter> Namespace<A> {
    pub fn new<C, T>(path: Cow<'static, str>, handler: C) -> Arc<Self>
    where
        C: ConnectHandler<A, T> + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        Arc::new_cyclic(|ns| Self {
            path,
            handler: MakeErasedHandler::new_ns_boxed(handler),
            sockets: HashMap::new().into(),
            adapter: A::new(ns.clone()),
        })
    }

    /// Connects a socket to a namespace.
    ///
    /// Middlewares are first called to check if the connection is allowed.
    /// * If the handler returns an error, a connect_error packet is sent to the client.
    /// * If the handler returns Ok, a connect packet is sent to the client
    /// and the handler is called.
    pub(crate) async fn connect(
        self: Arc<Self>,
        sid: Sid,
        esocket: Arc<engineioxide::Socket<SocketData>>,
        auth: Option<String>,
        config: Arc<SocketIoConfig>,
    ) -> Result<(), ConnectFail> {
        let socket: Arc<Socket<A>> = Socket::new(sid, self.clone(), esocket.clone(), config).into();

        if let Err(e) = self.handler.call_middleware(socket.clone(), &auth).await {
            #[cfg(feature = "tracing")]
            tracing::trace!(ns = self.path.as_ref(), ?socket.id, "emitting connect_error packet");

            let data = e.to_string();
            if let Err(_e) = socket.send(Packet::connect_error(&self.path, &data)) {
                #[cfg(feature = "tracing")]
                tracing::debug!("error sending connect_error packet: {:?}, closing conn", _e);
                esocket.close(engineioxide::DisconnectReason::PacketParsingError);
            }
            return Err(ConnectFail);
        }

        self.sockets.write().unwrap().insert(sid, socket.clone());
        #[cfg(feature = "tracing")]
        tracing::trace!(?socket.id, ?self.path, "socket added to namespace");

        let protocol = esocket.protocol.into();

        if let Err(_e) = socket.send(Packet::connect(&self.path, socket.id, protocol)) {
            #[cfg(feature = "tracing")]
            tracing::debug!("error sending connect packet: {:?}, closing conn", _e);
            esocket.close(engineioxide::DisconnectReason::PacketParsingError);
            return Err(ConnectFail);
        }

        socket.set_connected(true);
        self.handler.call(socket, auth);

        Ok(())
    }

    /// Removes a socket from a namespace and propagate the event to the adapter
    pub fn remove_socket(&self, sid: Sid) -> Result<(), AdapterError> {
        #[cfg(feature = "tracing")]
        tracing::trace!(?sid, "removing socket from namespace");

        self.sockets.write().unwrap().remove(&sid);
        self.adapter
            .del_all(sid)
            .map_err(|err| AdapterError(Box::new(err)))
    }

    pub fn has(&self, sid: Sid) -> bool {
        self.sockets.read().unwrap().values().any(|s| s.id == sid)
    }

    pub fn recv(&self, sid: Sid, packet: PacketData<'_>) -> Result<(), Error> {
        match packet {
            PacketData::Connect(_) => unreachable!("connect packets should be handled before"),
            PacketData::ConnectError(_) => Err(Error::InvalidPacketType),
            packet => self.get_socket(sid)?.recv(packet),
        }
    }

    pub fn get_socket(&self, sid: Sid) -> Result<Arc<Socket<A>>, Error> {
        self.sockets
            .read()
            .unwrap()
            .get(&sid)
            .cloned()
            .ok_or(Error::SocketGone(sid))
    }

    pub fn get_sockets(&self) -> Vec<Arc<Socket<A>>> {
        self.sockets.read().unwrap().values().cloned().collect()
    }

    /// Closes the entire namespace :
    /// * Closes the adapter
    /// * Closes all the sockets and their underlying connections
    /// * Removes all the sockets from the namespace
    pub async fn close(&self) {
        self.adapter.close().ok();
        #[cfg(feature = "tracing")]
        tracing::debug!("closing all sockets in namespace {}", self.path);
        let sockets = self.sockets.read().unwrap().clone();
        futures_util::future::join_all(sockets.values().map(|s| s.close_underlying_transport()))
            .await;
        self.sockets.write().unwrap().shrink_to_fit();
        #[cfg(feature = "tracing")]
        tracing::debug!("all sockets in namespace {} closed", self.path);
    }
}

#[cfg(any(test, socketioxide_test))]
impl<A: Adapter> Namespace<A> {
    pub fn new_dummy<const S: usize>(sockets: [Sid; S]) -> Arc<Self> {
        let ns = Namespace::new(Cow::Borrowed("/"), || {});
        for sid in sockets {
            ns.sockets
                .write()
                .unwrap()
                .insert(sid, Socket::new_dummy(sid, ns.clone()).into());
        }
        ns
    }

    pub fn clean_dummy_sockets(&self) {
        self.sockets.write().unwrap().clear();
    }
}

impl<A: Adapter + std::fmt::Debug> std::fmt::Debug for Namespace<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Namespace")
            .field("path", &self.path)
            .field("adapter", &self.adapter)
            .field("sockets", &self.sockets)
            .finish()
    }
}

#[cfg(feature = "tracing")]
impl<A: Adapter> Drop for Namespace<A> {
    fn drop(&mut self) {
        #[cfg(feature = "tracing")]
        tracing::debug!("dropping namespace {}", self.path);
    }
}
