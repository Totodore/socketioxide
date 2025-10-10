use std::time::Duration;

use socketioxide::{adapter::Adapter, extract::SocketRef};
mod fixture;

#[tokio::test]
pub async fn broadcast() {
    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        // delay to ensure all socket/servers are connected
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        socket.broadcast().emit("test", &2).await.unwrap();
    }

    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", handler).await.unwrap();
    io2.ns("/", handler).await.unwrap();

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx1), r#"42["test",2]"#);
    assert_eq!(timeout_rcv!(&mut rx2), r#"42["test",2]"#);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

#[tokio::test]
pub async fn broadcast_rooms() {
    let [io1, io2, io3] = fixture::spawn_servers();
    let handler = |room: &'static str, to: &'static str| {
        async move |socket: SocketRef<_>| {
            // delay to ensure all socket/servers are connected
            socket.join(room);
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            socket.to(to).emit("test", room).await.unwrap();
        }
    };

    io1.ns("/", handler("room1", "room2")).await.unwrap();
    io2.ns("/", handler("room2", "room3")).await.unwrap();
    io3.ns("/", handler("room3", "room1")).await.unwrap();

    let ((_tx1, mut rx1), (_tx2, mut rx2), (_tx3, mut rx3)) = tokio::join!(
        io1.new_dummy_sock("/", ()),
        io2.new_dummy_sock("/", ()),
        io3.new_dummy_sock("/", ())
    );

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet
    timeout_rcv!(&mut rx3); // Connect "/" packet

    // socket 1 is receiving a packet from io3
    assert_eq!(timeout_rcv!(&mut rx1), r#"42["test","room3"]"#);
    // socket 2 is receiving a packet from io2
    assert_eq!(timeout_rcv!(&mut rx2), r#"42["test","room1"]"#);
    // socket 3 is receiving a packet from io1
    assert_eq!(timeout_rcv!(&mut rx3), r#"42["test","room2"]"#);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
    timeout_rcv_err!(&mut rx3);
}

#[tokio::test]
pub async fn broadcast_with_ack() {
    use futures_util::stream::StreamExt;

    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        // delay to ensure all socket/servers are connected
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        socket
            .broadcast()
            .emit_with_ack::<_, String>("test", "bar")
            .await
            .unwrap()
            .for_each(async |(_, res)| {
                socket.emit("ack_res", &res).unwrap();
            })
            .await;
    }

    let [io1, io2] = fixture::spawn_servers();

    io1.ns("/", handler).await.unwrap();
    io2.ns("/", async || ()).await.unwrap();

    let ((_tx1, mut rx1), (tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx2), r#"421["test","bar"]"#);
    let packet_res = r#"431["foo"]"#.to_string().try_into().unwrap();
    tx2.try_send(packet_res).unwrap();
    assert_eq!(timeout_rcv!(&mut rx1), r#"42["ack_res",{"Ok":"foo"}]"#);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

#[tokio::test]
pub async fn broadcast_with_ack_timeout() {
    use futures_util::StreamExt;
    const TIMEOUT: Duration = Duration::from_millis(50);

    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        socket
            .broadcast()
            .emit_with_ack::<_, String>("test", "bar")
            .await
            .unwrap()
            .for_each(async |(_, res)| {
                socket.emit("ack_res", &res).unwrap();
            })
            .await;
        socket.emit("ack_res", "timeout").unwrap();
    }

    let [io1, io2] = fixture::spawn_buggy_servers(TIMEOUT);

    io1.ns("/", handler).await.unwrap();
    io2.ns("/", async || ()).await.unwrap();

    let now = std::time::Instant::now();
    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx2), r#"421["test","bar"]"#); // emit with ack message
    // We do not answer
    assert_eq!(
        timeout_rcv!(&mut rx1, TIMEOUT.as_millis() as u64 + 100),
        r#"42["ack_res","timeout"]"#
    );
    assert!(now.elapsed() >= TIMEOUT);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}
