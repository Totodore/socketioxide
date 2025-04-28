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
    SocketIo,
    adapter::Adapter,
    extract::{AckSender, Data, SocketRef},
    socket::Sid,
};
pub async fn handler<A: Adapter>(s: SocketRef<A>) {
    s.join(["room1", "room2", "room4", "room5"]);
    s.join(s.id);

    // "Broadcast" tests
    s.on("broadcast", broadcast);
    s.on("fetch_sockets", fetch_sockets);
    s.on("broadcast_with_ack", broadcast_with_ack);
    s.on("disconnect_socket", disconnect_socket);
    s.on("join_room", join_room);
    s.on("leave_room", leave_room);
    s.on("rooms", rooms);

    // Remote socket tests
    s.on("emit_from_remote_sock", emit_from_remote_sock);
    s.on(
        "emit_with_ack_from_remote_sock",
        emit_with_ack_from_remote_sock,
    );
    s.on("join_room_from_remote_sock", join_room_from_remote_sock);
    s.on("leave_room_from_remote_sock", leave_room_from_remote_sock);
    s.on("get_rooms_remote_sock", get_rooms_remote_sock);
    s.on("disconnect_remote_sock", disconnect_remote_sock);
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
async fn join_room<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    io.join("room7").await.unwrap();
    ack.send(&()).unwrap();
}
async fn leave_room<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    io.leave("room4").await.unwrap();
    ack.send(&()).unwrap();
}
async fn rooms<A: Adapter>(io: SocketIo<A>, ack: AckSender<A>) {
    let rooms = io.rooms().await.unwrap();
    ack.send(&rooms).unwrap();
}

async fn emit_from_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    s: SocketRef<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    sock.emit("emit", &format!("hello from {}", s.id))
        .await
        .unwrap();
    ack.send(&()).unwrap();
}
async fn emit_with_ack_from_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    s: SocketRef<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    let res: String = sock
        .emit_with_ack("emit", &format!("hello from {}", s.id))
        .await
        .expect("could not send ack")
        .await
        .expect("ack receive error");
    ack.send(&res).unwrap();
}
async fn join_room_from_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    s: SocketRef<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    sock.join(format!("hello from {}", s.id)).await.unwrap();
    ack.send(&()).unwrap();
}
async fn leave_room_from_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    sock.leave("room4").await.unwrap();
    ack.send(&()).unwrap();
}
async fn disconnect_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    sock.clone().disconnect().await.unwrap();
    ack.send(&()).unwrap();
}
async fn get_rooms_remote_sock<A: Adapter>(
    io: SocketIo<A>,
    Data(id): Data<Sid>,
    ack: AckSender<A>,
) {
    let socks = io.to(id).fetch_sockets().await.unwrap();
    assert_eq!(socks.len(), 1);
    let sock = socks.first().unwrap();
    let rooms = sock.rooms().await.unwrap();
    ack.send(&rooms).unwrap();
}
