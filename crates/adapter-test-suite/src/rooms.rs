//! Rooms tests shared by the remote adapters.

use std::time::Duration;

use socketioxide::extract::SocketRef;

use super::Fixture;

pub async fn all_rooms<F: Fixture>() {
    let [io1, io2, io3] = F::spawn_servers::<3>();
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);

    F::add_ns(&io1, handler(&["room1", "room2"])).await;
    F::add_ns(&io2, handler(&["room2", "room3"])).await;
    F::add_ns(&io3, handler(&["room3", "room1"])).await;

    let ((_tx1, mut rx1), (_tx2, mut rx2), (_tx3, mut rx3)) = tokio::join!(
        io1.new_dummy_sock("/", ()),
        io2.new_dummy_sock("/", ()),
        io3.new_dummy_sock("/", ())
    );

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet
    timeout_rcv!(&mut rx3); // Connect "/" packet

    const ROOMS: [&str; 3] = ["room1", "room2", "room3"];
    for io in [io1, io2, io3] {
        let mut rooms = io.rooms().await.unwrap();
        rooms.sort();
        assert_eq!(rooms, ROOMS);
    }

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
    timeout_rcv_err!(&mut rx3);
}

pub async fn all_rooms_timeout<F: Fixture>() {
    const TIMEOUT: Duration = Duration::from_millis(50);
    let [io1, io2, io3] = F::spawn_buggy_servers::<3>(TIMEOUT);
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);

    F::add_ns(&io1, handler(&["room1", "room2"])).await;
    F::add_ns(&io2, handler(&["room2", "room3"])).await;
    F::add_ns(&io3, handler(&["room3", "room1"])).await;

    let ((_tx1, mut rx1), (_tx2, mut rx2), (_tx3, mut rx3)) = tokio::join!(
        io1.new_dummy_sock("/", ()),
        io2.new_dummy_sock("/", ()),
        io3.new_dummy_sock("/", ())
    );

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet
    timeout_rcv!(&mut rx3); // Connect "/" packet

    const ROOMS: [&str; 3] = ["room1", "room2", "room3"];
    for io in [io1, io2, io3] {
        let now = std::time::Instant::now();
        let mut rooms = io.rooms().await.unwrap();
        assert!(now.elapsed() >= TIMEOUT); // timeout time
        rooms.sort();
        assert_eq!(rooms, ROOMS);
    }

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
    timeout_rcv_err!(&mut rx3);
}

pub async fn add_sockets<F: Fixture>() {
    let handler = |room: &'static str| async move |socket: SocketRef<_>| socket.join(room);
    let [io1, io2] = F::spawn_servers::<2>();

    F::add_ns(&io1, handler("room1")).await;
    F::add_ns(&io2, handler("room3")).await;

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet
    io1.broadcast().join("room2").await.unwrap();
    let mut rooms = io1.rooms().await.unwrap();
    rooms.sort();
    assert_eq!(rooms, ["room1", "room2", "room3"]);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

pub async fn del_sockets<F: Fixture>() {
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);
    let [io1, io2] = F::spawn_servers::<2>();

    F::add_ns(&io1, handler(&["room1", "room2"])).await;
    F::add_ns(&io2, handler(&["room3", "room2"])).await;

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    io1.broadcast().leave("room2").await.unwrap();

    let mut rooms = io1.rooms().await.unwrap();
    rooms.sort();
    assert_eq!(rooms, ["room1", "room3"]);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}
