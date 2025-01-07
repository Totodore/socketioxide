//! Test all adapter methods.
//!
//! There are 7 types of requests:
//! * Broadcast a packet to all the matching sockets.
//! * Broadcast a packet to all the matching sockets and wait for a stream of acks.
//! * Disconnect matching sockets.
//! * Get all the rooms.
//! * Add matching sockets to rooms.
//! * Remove matching sockets to rooms.
//! * Fetch all the remote sockets matching the options.

use futures_util::StreamExt;
use socketioxide::{
    adapter::Adapter,
    extract::{AckSender, SocketRef},
    SocketIo,
};
pub async fn handler<A: Adapter>(s: SocketRef<A>) {
    s.join(["room1", "room2", "room4", "room5"]);
    s.join(s.id);
    s.on("broadcast", broadcast);
    s.on("fetch_sockets", fetch_sockets);
    s.on("broadcast_with_ack", broadcast_with_ack);
    s.on("disconnect_socket", disconnect_socket);
    s.on("rooms", rooms);
}

async fn broadcast<A: Adapter>(io: SocketIo<A>, s: SocketRef<A>) {
    io.emit("broadcast", &format!("hello from {}", s.id))
        .await
        .unwrap();
}
async fn broadcast_with_ack<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    let data: Vec<String> = io
        .emit_with_ack("broadcast_with_ack", &())
        .await
        .unwrap()
        .map(|(_, d)| d.unwrap())
        .collect()
        .await;
    ack.send(&data).unwrap();
}
async fn fetch_sockets<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    let sockets: Vec<_> = io
        .fetch_sockets()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.into_data())
        .collect();
    ack.send(&sockets).unwrap();
}
async fn disconnect_socket<A: Adapter>(io: SocketIo<A>) {
    io.disconnect().await.unwrap();
}
async fn rooms<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    let rooms = io.rooms().await.unwrap();
    ack.send(&rooms).unwrap();
}
