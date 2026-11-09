//! Broadcast tests shared by the remote adapters.

use std::time::Duration;

use engineioxide::{Packet, ProtocolVersion};
use socketioxide::{adapter::Adapter, extract::SocketRef};

use super::Fixture;

pub async fn broadcast<F: Fixture>() {
    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        // delay to ensure all socket/servers are connected
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        socket.broadcast().emit("test", &2).await.unwrap();
    }

    let [io1, io2] = F::spawn_servers::<2>();

    F::add_ns(&io1, handler).await;
    F::add_ns(&io2, handler).await;

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx1), r#"42["test",2]"#);
    assert_eq!(timeout_rcv!(&mut rx2), r#"42["test",2]"#);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

pub async fn broadcast_rooms<F: Fixture>() {
    let [io1, io2, io3] = F::spawn_servers::<3>();
    let handler = |room: &'static str, to: &'static str| {
        async move |socket: SocketRef<_>| {
            // delay to ensure all socket/servers are connected
            socket.join(room);
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            socket.to(to).emit("test", room).await.unwrap();
        }
    };

    F::add_ns(&io1, handler("room1", "room2")).await;
    F::add_ns(&io2, handler("room2", "room3")).await;
    F::add_ns(&io3, handler("room3", "room1")).await;

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

pub async fn broadcast_with_ack<F: Fixture>() {
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

    let [io1, io2] = F::spawn_servers::<2>();

    F::add_ns(&io1, handler).await;
    F::add_ns(&io2, async || ()).await;

    let ((_tx1, mut rx1), (tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx2), r#"421["test","bar"]"#);
    let packet_res = Packet::parse(ProtocolVersion::V4, r#"431["foo"]"#).unwrap();
    tx2.try_send(packet_res).unwrap();
    assert_eq!(timeout_rcv!(&mut rx1), r#"42["ack_res",{"Ok":"foo"}]"#);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

pub async fn broadcast_with_ack_timeout<F: Fixture>() {
    use futures_util::StreamExt;
    const REQ_TIMEOUT: Duration = Duration::from_millis(50);
    const ACK_TIMEOUT: Duration = Duration::from_millis(50);
    const TIMEOUT: Duration = Duration::from_millis(100);

    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        socket
            .broadcast()
            .timeout(ACK_TIMEOUT)
            .emit_with_ack::<_, String>("test", "bar")
            .await
            .unwrap()
            .for_each(async |(_, res)| {
                socket.emit("ack_res", &res).unwrap();
            })
            .await;
        socket.emit("ack_res", "timeout").unwrap();
    }

    let [io1, io2] = F::spawn_buggy_servers::<2>(REQ_TIMEOUT);

    F::add_ns(&io1, handler).await;
    F::add_ns(&io2, async || ()).await;

    let now = std::time::Instant::now();
    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    assert_eq!(timeout_rcv!(&mut rx2), r#"421["test","bar"]"#); // emit with ack message
    // We do not answer: the remote node applies the timeout of the caller and reports it
    assert_eq!(
        timeout_rcv!(&mut rx1, TIMEOUT.as_millis() as u64 + 100),
        r#"42["ack_res",{"Err":"Timeout"}]"#
    );
    assert_eq!(
        timeout_rcv!(&mut rx1, TIMEOUT.as_millis() as u64 + 100),
        r#"42["ack_res","timeout"]"#
    );
    // At least the ack timeout: the stream can end sooner than `TIMEOUT` now that
    // the remote node reports its timeout, if no other node is expected.
    assert!(now.elapsed() >= ACK_TIMEOUT);

    timeout_rcv_err!(&mut rx1);
    timeout_rcv_err!(&mut rx2);
}

/// The timeout given by the caller applies to the sockets of every node.
pub async fn emit_with_ack_timeout_applies_to_remote_sockets<F: Fixture>() {
    use futures_util::StreamExt;

    let [io1, io2] = F::spawn_servers::<2>();
    F::add_ns(&io1, async || ()).await;
    F::add_ns(&io2, async || ()).await;
    let ((_tx1, _rx1), (_tx2, _rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));
    tokio::time::sleep(Duration::from_millis(20)).await;

    // None of the clients answer. The default ack timeout of a node is 5 seconds.
    let stream = io1
        .timeout(Duration::from_millis(50))
        .emit_with_ack::<_, String>("test", &1)
        .await
        .unwrap();
    let results: Vec<_> = tokio::time::timeout(Duration::from_secs(1), stream.collect())
        .await
        .expect("the remote node should apply the timeout of the caller");

    assert_eq!(results.len(), 2);
    for (_, res) in results {
        assert!(res.is_err());
    }
}
