//! Shared remote request handling for remote adapters.
//!
//! The [`RemoteRequestHandler`] trait provides default implementations for the
//! requests a node can receive from another node. Adapters only have to provide
//! access to their local adapter and a way to send responses back.

use std::{fmt, future::Future, sync::Arc};

use futures_util::StreamExt;
use serde::Serialize;

use crate::{
    Sid, Uid,
    adapter::{
        BroadcastOptions, CoreLocalAdapter, Room, SocketEmitter,
        errors::AdapterError,
        remote_packet::{RequestIn, RequestTypeIn, Response, ResponseType},
    },
    packet::Packet,
};

/// Handle requests received from other nodes of the cluster.
///
/// Every remote adapter implements this trait. The default methods dispatch the
/// request to the right handler and apply it on the local adapter.
pub trait RemoteRequestHandler<E: SocketEmitter>: Send + Sync + 'static {
    /// The error returned when sending a response.
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;

    /// The local adapter, used to apply the requests on the local sockets.
    fn local(&self) -> &CoreLocalAdapter<E>;

    /// Send a response to the node that sent the request.
    fn send_res<T: Serialize + fmt::Debug + Send + 'static>(
        &self,
        req_id: Sid,
        req_origin: Uid,
        res: Response<T>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static;

    /// Handle a heartbeat request received from a remote node.
    ///
    /// It is a no-op by default as some adapters do not rely on heartbeats to
    /// track the liveness of the other nodes.
    fn recv_heartbeat(self: &Arc<Self>, req_type: RequestTypeIn, origin: Uid) {
        let _ = (self, req_type, origin);
    }

    /// Handle a request received from another node.
    fn recv_req(self: &Arc<Self>, req: RequestIn) {
        let RequestIn {
            node_id,
            id,
            r#type,
            opts,
        } = req;

        // Ignore loopback requests.
        if node_id == self.local().server_id() {
            return;
        }

        tracing::trace!(?r#type, ?id, ?node_id, "incoming request");
        match (r#type, opts) {
            (req_type @ (RequestTypeIn::Heartbeat | RequestTypeIn::InitHeartbeat), _) => {
                Self::recv_heartbeat(self, req_type, node_id);
            }
            (r#type, Some(opts)) => match r#type {
                RequestTypeIn::Broadcast(p) => self.recv_broadcast(opts, p),
                RequestTypeIn::BroadcastWithAck(p) => {
                    self.clone().recv_broadcast_with_ack(node_id, id, p, opts)
                }
                RequestTypeIn::DisconnectSockets => self.recv_disconnect_sockets(opts),
                RequestTypeIn::AllRooms => self.recv_rooms(node_id, id, opts),
                RequestTypeIn::AddSockets(rooms) => self.recv_add_sockets(opts, rooms),
                RequestTypeIn::DelSockets(rooms) => self.recv_del_sockets(opts, rooms),
                RequestTypeIn::FetchSockets => self.recv_fetch_sockets(node_id, id, opts),
                RequestTypeIn::Heartbeat | RequestTypeIn::InitHeartbeat => unreachable!(),
            },
            (r#type, None) => {
                tracing::warn!(?node_id, ?r#type, "request is missing options");
            }
        }
    }

    /// Broadcast a packet to the local sockets matching the options.
    fn recv_broadcast(&self, opts: BroadcastOptions, packet: Packet) {
        if let Err(e) = self.local().broadcast(packet, opts) {
            let ns = self.local().path();
            let node_id = self.local().server_id();
            tracing::warn!(
                %node_id,
                ?ns,
                "remote request broadcast handler: {:?}",
                e
            );
        }
    }

    /// Disconnect the local sockets matching the options.
    fn recv_disconnect_sockets(&self, opts: BroadcastOptions) {
        if let Err(e) = self.local().disconnect_socket(opts) {
            let ns = self.local().path();
            let node_id = self.local().server_id();
            tracing::warn!(
                %node_id,
                ?ns,
                "remote request disconnect sockets handler: {:?}",
                e
            );
        }
    }

    /// Broadcast a packet to the local sockets matching the options and send back
    /// the expected ack count and the acks as they are received.
    fn recv_broadcast_with_ack(
        self: Arc<Self>,
        origin: Uid,
        req_id: Sid,
        packet: Packet,
        opts: BroadcastOptions,
    ) {
        let node_id = self.local().server_id();
        let (stream, count) = self.local().broadcast_with_ack(packet, opts, None);
        tokio::spawn(async move {
            let on_err = |err| {
                let ns = self.local().path();
                let node_id = self.local().server_id();
                tracing::warn!(
                    %node_id,
                    ?ns,
                    "remote request broadcast with ack handler errors: {:?}",
                    err
                );
            };
            // First send the count of expected acks to the server that sent the request.
            // This is used to keep track of the number of expected acks.
            let res = Response {
                r#type: ResponseType::<()>::BroadcastAckCount(count),
                node_id,
            };
            if let Err(err) = self.send_res(req_id, origin, res).await {
                on_err(err);
                return;
            }

            // Then send the acks as they are received.
            futures_util::pin_mut!(stream);
            while let Some(ack) = stream.next().await {
                let res = Response {
                    r#type: ResponseType::BroadcastAck(ack),
                    node_id,
                };
                if let Err(err) = self.send_res(req_id, origin, res).await {
                    on_err(err);
                    return;
                }
            }
        });
    }

    /// Send back all the local room names.
    fn recv_rooms(&self, origin: Uid, req_id: Sid, opts: BroadcastOptions) {
        let rooms = self.local().rooms(opts);
        let res = Response {
            r#type: ResponseType::<()>::AllRooms(rooms),
            node_id: self.local().server_id(),
        };
        let fut = self.send_res(req_id, origin, res);
        let ns = self.local().path().clone();
        let uid = self.local().server_id();
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request rooms handler: {:?}", err);
            }
        });
    }

    /// Add the local sockets matching the options to the rooms.
    fn recv_add_sockets(&self, opts: BroadcastOptions, rooms: Vec<Room>) {
        self.local().add_sockets(opts, rooms);
    }

    /// Remove the local sockets matching the options from the rooms.
    fn recv_del_sockets(&self, opts: BroadcastOptions, rooms: Vec<Room>) {
        self.local().del_sockets(opts, rooms);
    }

    /// Send back the data of all the local sockets matching the options.
    fn recv_fetch_sockets(&self, origin: Uid, req_id: Sid, opts: BroadcastOptions) {
        let sockets = self.local().fetch_sockets(opts);
        let res = Response {
            node_id: self.local().server_id(),
            r#type: ResponseType::FetchSockets(sockets),
        };
        let fut = self.send_res(req_id, origin, res);
        let ns = self.local().path().clone();
        let uid = self.local().server_id();
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::warn!(?uid, ?ns, "remote request fetch sockets handler: {:?}", err);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use tokio::sync::mpsc;

    use super::*;
    use crate::{
        Value,
        adapter::{
            errors::AdapterError,
            test::{StubSockets, create_adapter},
        },
    };

    struct TestHandler {
        local: CoreLocalAdapter<StubSockets>,
        responses: mpsc::UnboundedSender<String>,
        heartbeats: AtomicUsize,
    }

    impl RemoteRequestHandler<StubSockets> for TestHandler {
        type Error = AdapterError;

        fn local(&self) -> &CoreLocalAdapter<StubSockets> {
            &self.local
        }

        fn send_res<T: Serialize + fmt::Debug + Send + 'static>(
            &self,
            _req_id: Sid,
            _req_origin: Uid,
            res: Response<T>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let tx = self.responses.clone();
            async move {
                tx.send(format!("{res:?}")).ok();
                Ok(())
            }
        }

        fn recv_heartbeat(self: &Arc<Self>, _req_type: RequestTypeIn, _origin: Uid) {
            self.heartbeats.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn handler<const S: usize>(
        sockets: [Sid; S],
    ) -> (Arc<TestHandler>, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handler = TestHandler {
            local: create_adapter(sockets),
            responses: tx,
            heartbeats: AtomicUsize::new(0),
        };
        (Arc::new(handler), rx)
    }

    fn req(r#type: RequestTypeIn, opts: Option<BroadcastOptions>) -> RequestIn {
        RequestIn {
            node_id: Uid::new(),
            id: Sid::new(),
            r#type,
            opts,
        }
    }

    #[test]
    fn heartbeat_hook_is_called() {
        let (handler, _rx) = handler([Sid::new()]);
        handler.recv_req(req(RequestTypeIn::InitHeartbeat, None));
        assert_eq!(handler.heartbeats.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn loopback_request_is_ignored() {
        let (handler, _rx) = handler([Sid::new()]);
        let mut req = req(RequestTypeIn::InitHeartbeat, None);
        req.node_id = Uid::ZERO;
        handler.recv_req(req);
        assert_eq!(handler.heartbeats.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn add_and_del_sockets() {
        let sid = Sid::new();
        let (handler, _rx) = handler([sid]);
        let opts = BroadcastOptions::new(sid);

        handler.recv_req(req(
            RequestTypeIn::AddSockets(vec!["room1".into()]),
            Some(opts.clone()),
        ));
        assert!(handler.local().socket_rooms(sid).contains("room1"));

        handler.recv_req(req(
            RequestTypeIn::DelSockets(vec!["room1".into()]),
            Some(opts),
        ));
        assert!(handler.local().socket_rooms(sid).is_empty());
    }

    #[test]
    fn request_without_opts_is_skipped() {
        let (handler, mut rx) = handler([Sid::new()]);
        handler.recv_req(req(RequestTypeIn::AllRooms, None));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn rooms_response_is_sent() {
        let sid = Sid::new();
        let (handler, mut rx) = handler([sid]);
        handler.local().add_all(sid, ["room1"]);
        handler.recv_req(req(
            RequestTypeIn::AllRooms,
            Some(BroadcastOptions::new(sid)),
        ));

        let res = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(res.contains("AllRooms"), "unexpected response: {res}");
    }

    #[tokio::test]
    async fn broadcast_with_ack_count_is_sent() {
        let sid = Sid::new();
        let (handler, mut rx) = handler([sid]);
        let packet = Packet::event("/", Value::Str("test".into(), None));
        handler.recv_req(req(
            RequestTypeIn::BroadcastWithAck(packet),
            Some(BroadcastOptions::new(sid)),
        ));

        let res = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            res.contains("BroadcastAckCount"),
            "unexpected response: {res}"
        );
    }
}
