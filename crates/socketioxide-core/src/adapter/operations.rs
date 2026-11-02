//! Shared outbound operations for remote adapters.
//!
//! The [`RemoteOperations`] trait provides default implementations for the
//! operations that are forwarded to the other nodes of the cluster. Adapters
//! only have to provide the transport specific parts: sending a request and
//! waiting for its responses.

use std::{fmt, future::Future};

use futures_core::Stream;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::{
    Sid, Uid,
    adapter::{
        BroadcastOptions, CoreLocalAdapter, RemoteSocketData, Room, RoomParam, SocketEmitter,
        errors::{AdapterError, BroadcastError},
        remote_packet::{RequestOut, RequestTypeOut, Response, ResponseTypeId},
    },
    packet::Packet,
};

/// Operations forwarded to the other nodes of the cluster.
///
/// Every remote adapter implements this trait. The default methods send the
/// request to the other nodes and then apply the operation on the local adapter.
pub trait RemoteOperations<E: SocketEmitter>: Send + Sync + 'static {
    /// The error returned when sending a request or waiting for its responses.
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;

    /// The local adapter, used to apply the operations on the local sockets.
    fn local(&self) -> &CoreLocalAdapter<E>;

    /// The unique identifier of the current node.
    fn uid(&self) -> Uid;

    /// The number of servers in the cluster, including the current one.
    fn server_count(&self) -> impl Future<Output = Result<u16, Self::Error>> + Send;

    /// Send a request to a specific target node or broadcast it to all nodes
    /// if no target is specified.
    fn send_req(
        &self,
        req: RequestOut<'_>,
        target: Option<Uid>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Await for the responses of a request sent to the other nodes.
    fn get_res<D: DeserializeOwned + fmt::Debug + Send + 'static>(
        &self,
        req_id: Sid,
        response_type: ResponseTypeId,
        target: Option<Uid>,
    ) -> impl Future<Output = Result<impl Stream<Item = Response<D>> + Send, Self::Error>> + Send;

    /// Broadcast a packet to the sockets matching the options, on every node.
    fn broadcast(
        &self,
        packet: Packet,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        async move {
            let uid = self.uid();
            if !opts.is_local(uid) {
                let req = RequestOut::new(uid, RequestTypeOut::Broadcast(&packet), &opts);
                self.send_req(req, opts.server_id)
                    .await
                    .map_err(Into::<AdapterError>::into)?;
            }

            self.local().broadcast(packet, opts)?;
            Ok(())
        }
    }

    /// Disconnect the sockets matching the options, on every node.
    fn disconnect_socket(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<(), BroadcastError>> + Send {
        async move {
            let uid = self.uid();
            if !opts.is_local(uid) {
                let req = RequestOut::new(uid, RequestTypeOut::DisconnectSockets, &opts);
                self.send_req(req, opts.server_id)
                    .await
                    .map_err(Into::<AdapterError>::into)?;
            }

            self.local()
                .disconnect_socket(opts)
                .map_err(BroadcastError::Socket)?;
            Ok(())
        }
    }

    /// Get all the room names of the sockets matching the options, on every node.
    fn rooms(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<Room>, Self::Error>> + Send {
        async move {
            let uid = self.uid();
            if opts.is_local(uid) {
                return Ok(self.local().rooms(opts).into_iter().collect());
            }
            let req = RequestOut::new(uid, RequestTypeOut::AllRooms, &opts);
            let req_id = req.id;

            // First get the remote stream because the backend might send
            // the responses before the subscription is done.
            let stream = self
                .get_res::<()>(req_id, ResponseTypeId::AllRooms, opts.server_id)
                .await?;
            self.send_req(req, opts.server_id).await?;

            let local = self.local().rooms(opts);
            let rooms = stream
                .filter_map(|item| std::future::ready(item.into_rooms()))
                .fold(local, async |mut acc, item| {
                    acc.extend(item);
                    acc
                })
                .await;
            Ok(Vec::from_iter(rooms))
        }
    }

    /// Add the sockets matching the options to the rooms, on every node.
    fn add_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // Collected before creating the future so it does not have to be `Send`.
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        async move {
            let uid = self.uid();
            if !opts.is_local(uid) {
                let req = RequestOut::new(uid, RequestTypeOut::AddSockets(&rooms), &opts);
                self.send_req(req, opts.server_id).await?;
            }

            self.local().add_sockets(opts, rooms);
            Ok(())
        }
    }

    /// Remove the sockets matching the options from the rooms, on every node.
    fn del_sockets(
        &self,
        opts: BroadcastOptions,
        rooms: impl RoomParam,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // Collected before creating the future so it does not have to be `Send`.
        let rooms: Vec<Room> = rooms.into_room_iter().collect();
        async move {
            let uid = self.uid();
            if !opts.is_local(uid) {
                let req = RequestOut::new(uid, RequestTypeOut::DelSockets(&rooms), &opts);
                self.send_req(req, opts.server_id).await?;
            }

            self.local().del_sockets(opts, rooms);
            Ok(())
        }
    }

    /// Get the data of the sockets matching the options, on every node.
    fn fetch_sockets(
        &self,
        opts: BroadcastOptions,
    ) -> impl Future<Output = Result<Vec<RemoteSocketData>, Self::Error>> + Send {
        async move {
            let uid = self.uid();
            if opts.is_local(uid) {
                return Ok(self.local().fetch_sockets(opts));
            }
            let req = RequestOut::new(uid, RequestTypeOut::FetchSockets, &opts);

            // First get the remote stream because the backend might send
            // the responses before the subscription is done.
            let remote = self
                .get_res::<RemoteSocketData>(req.id, ResponseTypeId::FetchSockets, opts.server_id)
                .await?;
            self.send_req(req, opts.server_id).await?;

            let local = self.local().fetch_sockets(opts);
            let sockets = remote
                .filter_map(|item| std::future::ready(item.into_fetch_sockets()))
                .fold(local, async |mut acc, item| {
                    acc.extend(item);
                    acc
                })
                .await;
            Ok(sockets)
        }
    }
}
