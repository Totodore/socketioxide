use std::time::Duration;

use socketioxide::extract::SocketRef;

mod fixture;

#[tokio::test]
pub async fn all_rooms() {
    let [io1, io2, io3] = fixture::spawn_servers();
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);

    io1.ns("/", handler(&["room1", "room2"])).await.unwrap();
    io2.ns("/", handler(&["room2", "room3"])).await.unwrap();
    io3.ns("/", handler(&["room3", "room1"])).await.unwrap();

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

#[tokio::test]
pub async fn all_rooms_timeout() {
    const TIMEOUT: Duration = Duration::from_millis(50);
    let [io1, io2, io3] = fixture::spawn_buggy_servers(TIMEOUT);
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);

    io1.ns("/", handler(&["room1", "room2"])).await.unwrap();
    io2.ns("/", handler(&["room2", "room3"])).await.unwrap();
    io3.ns("/", handler(&["room3", "room1"])).await.unwrap();

    let ((_tx1, mut rx1), (_tx2, mut rx2), (_tx3, mut rx3)) = tokio::join!(
        io1.new_dummy_sock("/", ()),
        io2.new_dummy_sock("/", ()),
        io3.new_dummy_sock("/", ())
    );

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet
    timeout_rcv!(&mut rx3); // Connect "/" packet

    const ROOMS: [&str; 3] = ["room1", "room2", "room3"];
    for io in [io1, io3, io2] {
        let now = std::time::Instant::now();
        let mut rooms = io.rooms().await.unwrap();
        dbg!(&rooms);
        assert!(dbg!(now.elapsed()) >= TIMEOUT); // timeout time
        rooms.sort();
        assert_eq!(rooms, ROOMS);
    }

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
    timeout_rcv_err!(&mut rx3);
}
#[tokio::test]
pub async fn add_sockets() {
    let handler = |room: &'static str| async move |socket: SocketRef<_>| socket.join(room);
    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", handler("room1")).await.unwrap();
    io2.ns("/", handler("room3")).await.unwrap();

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

#[tokio::test]
pub async fn del_sockets() {
    let handler =
        |rooms: &'static [&'static str]| async move |socket: SocketRef<_>| socket.join(rooms);
    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", handler(&["room1", "room2"])).await.unwrap();
    io2.ns("/", handler(&["room3", "room2"])).await.unwrap();

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
