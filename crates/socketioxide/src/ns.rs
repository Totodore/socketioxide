use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    adapter::Adapter,
    errors::{ConnectFail, Error},
    handler::{BoxedConnectHandler, ConnectHandler, MakeErasedHandler},
    packet::{ConnectPacket, Packet, PacketData},
    socket::{DisconnectReason, Socket},
    ProtocolVersion,
};
use crate::{client::SocketData, errors::AdapterError};
use engineioxide::{sid::Sid, Str};
use socketioxide_core::{parser::Parse, Value};

/// A [`Namespace`] constructor used for dynamic namespaces
/// A namespace constructor only hold a common handler that will be cloned
/// to the instantiated namespaces.
pub struct NamespaceCtr<A: Adapter> {
    handler: BoxedConnectHandler<A>,
}
pub struct Namespace<A: Adapter> {
    pub path: Str,
    pub(crate) adapter: A,
    handler: BoxedConnectHandler<A>,
    sockets: RwLock<HashMap<Sid, Arc<Socket<A>>>>,
}

/// ===== impl NamespaceCtr =====
impl<A: Adapter> NamespaceCtr<A> {
    pub fn new<C, T>(handler: C) -> Self
    where
        C: ConnectHandler<A, T> + Send + Sync + 'static,
        T: Send + Sync + 'static,
    {
        Self {
            handler: MakeErasedHandler::new_ns_boxed(handler),
        }
    }
    pub fn get_new_ns(&self, path: Str) -> Arc<Namespace<A>> {
        Arc::new_cyclic(|ns| Namespace {
            path,
            handler: self.handler.boxed_clone(),
            sockets: HashMap::new().into(),
            adapter: A::new(ns.clone()),
        })
    }
}

impl<A: Adapter> Namespace<A> {
    pub fn new<C, T>(path: Str, handler: C) -> Arc<Self>
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
        esocket: Arc<engineioxide::Socket<SocketData<A>>>,
        auth: Option<Value>,
    ) -> Result<(), ConnectFail> {
        let socket: Arc<Socket<A>> = Socket::new(sid, self.clone(), esocket.clone()).into();

        if let Err(e) = self.handler.call_middleware(socket.clone(), &auth).await {
            #[cfg(feature = "tracing")]
            tracing::trace!(ns = self.path.as_str(), ?socket.id, "emitting connect_error packet");

            let data = e.to_string();
            if let Err(_e) = socket.send(Packet::connect_error(self.path.clone(), &data)) {
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
        let payload = ConnectPacket { sid: socket.id };
        let payload = match protocol {
            ProtocolVersion::V5 => Some(socket.parser().encode_default(&payload).unwrap()),
            _ => None,
        };
        if let Err(_e) = socket.send(Packet::connect(self.path.clone(), payload)) {
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

    pub fn recv(&self, sid: Sid, packet: PacketData) -> Result<(), Error> {
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
    /// * Closes all the sockets and
    /// their underlying connections in case of [`DisconnectReason::ClosingServer`]
    /// * Removes all the sockets from the namespace
    ///
    /// This function is using .await points only when called with [`DisconnectReason::ClosingServer`]
    pub async fn close(&self, reason: DisconnectReason) {
        use futures_util::future;
        let sockets = self.sockets.read().unwrap().clone();

        #[cfg(feature = "tracing")]
        tracing::debug!(?self.path, "closing {} sockets in namespace", sockets.len());

        if reason == DisconnectReason::ClosingServer {
            // When closing the underlying transport, this will indirectly close the socket
            // Therefore there is no need to manually call `s.close()`.
            future::join_all(sockets.values().map(|s| s.close_underlying_transport())).await;
        } else {
            for s in sockets.into_values() {
                let _sid = s.id;
                let _err = s.close(reason);
                #[cfg(feature = "tracing")]
                if let Err(err) = _err {
                    tracing::debug!(?_sid, ?err, "error closing socket");
                }
            }
        }
        #[cfg(feature = "tracing")]
        tracing::debug!(?self.path, "all sockets in namespace closed");

        let _err = self.adapter.close();
        #[cfg(feature = "tracing")]
        if let Err(err) = _err {
            tracing::debug!(?err, "could not close adapter");
        }
    }
}

#[doc(hidden)]
#[cfg(feature = "__test_harness")]
impl<A: Adapter> Namespace<A> {
    pub fn new_dummy<const S: usize>(sockets: [Sid; S]) -> Arc<Self> {
        let ns = Namespace::new("/".into(), || {});
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
