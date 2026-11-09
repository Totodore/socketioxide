//! Sockets tests shared by the remote adapters.
mod fixture;

#[tokio::test]
async fn fetch_sockets() {
    adapter_test_suite::sockets::fetch_sockets::<fixture::Fixture>().await;
}

#[tokio::test]
async fn fetch_sockets_with_rooms() {
    adapter_test_suite::sockets::fetch_sockets_with_rooms::<fixture::Fixture>().await;
}

#[tokio::test]
async fn fetch_sockets_timeout() {
    adapter_test_suite::sockets::fetch_sockets_timeout::<fixture::Fixture>().await;
}

#[tokio::test]
async fn remote_socket_emit() {
    adapter_test_suite::sockets::remote_socket_emit::<fixture::Fixture>().await;
}

#[tokio::test]
async fn remote_socket_emit_with_ack() {
    adapter_test_suite::sockets::remote_socket_emit_with_ack::<fixture::Fixture>().await;
}

#[tokio::test]
async fn remote_socket_emit_with_ack_targeted_terminates() {
    use adapter_test_suite::timeout_rcv;

    // A targeted ack request is only sent to the server owning the socket, so
    // the stream must terminate as soon as this server answered. It only
    // manifests with 3+ servers, as with 2 servers the targeted count is
    // equal to `server_count - 1`.
    let [io1, io2, io3] = fixture::spawn_servers::<3>();

    io1.ns("/", async || ()).await.unwrap();
    io2.ns("/", async || ()).await.unwrap();
    io3.ns("/", async || ()).await.unwrap();

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (stx2, mut rx2) = io2.new_dummy_sock("/", ()).await;
    let (stx3, mut rx3) = io3.new_dummy_sock("/", ()).await;

    timeout_rcv!(&mut rx1); // connect packet
    timeout_rcv!(&mut rx2); // connect packet
    timeout_rcv!(&mut rx3); // connect packet

    use futures_util::StreamExt;

    let local_id = io1.config().server_id;
    let io2_id = io2.config().server_id;
    let mut tested = 0;
    let sockets = io1.fetch_sockets().await.unwrap();
    for socket in sockets {
        // Only the remote sockets take the targeted network path.
        if socket.data().server_id == local_id {
            continue;
        }
        let (stx, rx) = if socket.data().server_id == io2_id {
            (&stx2, &mut rx2)
        } else {
            (&stx3, &mut rx3)
        };

        let stream = socket
            .emit_with_ack::<_, [String; 1]>("test", "hello")
            .await
            .unwrap();

        // Make the remote client answer the ack.
        let packet = timeout_rcv!(rx, 1000); // "42<ack_id>[\"test\",\"hello\"]"
        let ack_id = &packet[2..packet.find('[').unwrap()];
        stx.send(engineioxide::Packet::Message(
            format!("3{ack_id}[\"oof\"]").into(),
        ))
        .await
        .unwrap();

        futures_util::pin_mut!(stream);
        let ack = stream.next().await;
        assert!(matches!(ack, Some((_, Ok(_)))), "expected the ack");
        assert!(
            futures_core::FusedStream::is_terminated(&stream),
            "the ack stream should terminate right after the targeted ack"
        );
        tested += 1;
    }
    assert!(tested > 0, "expected remote sockets to be tested");
}
