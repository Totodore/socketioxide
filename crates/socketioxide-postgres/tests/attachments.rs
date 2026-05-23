//! Tests exercising the attachment path: small payloads stay inline, payloads above
//! `payload_threshold` are pushed to the attachment table and fetched by the receiver.
//!
//! The stub driver in `fixture.rs` tracks push/fetch activity per server, so every test
//! asserts both a functional outcome (the message arrived) and a pipeline outcome (an
//! attachment row was actually used).

use std::time::Duration;

use socketioxide::{adapter::Adapter, extract::SocketRef};
use socketioxide_postgres::PostgresAdapterConfig;

mod fixture;

const LOW_THRESHOLD: usize = 128;
const HIGH_THRESHOLD: usize = 10_000_000;

fn filler(len: usize) -> String {
    "x".repeat(len)
}

fn low_threshold_config() -> PostgresAdapterConfig {
    PostgresAdapterConfig::default().with_payload_threshold(LOW_THRESHOLD)
}

/// With a very high threshold, no payload should ever land in the attachment table.
/// Guards against a regression that routes everything through `push_attachment`.
#[tokio::test]
async fn broadcast_below_threshold_is_inline() {
    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let msg = "a".repeat(512);
        socket.broadcast().emit("test", &msg).await.unwrap();
    }

    let config = PostgresAdapterConfig::default().with_payload_threshold(HIGH_THRESHOLD);
    let ([io1, io2], [h1, h2]) = fixture::spawn_servers_with_handles::<2>(config);

    io1.ns("/", handler).await.unwrap();
    io2.ns("/", handler).await.unwrap();

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    // Both servers broadcast on connect; each remote socket should receive exactly one message.
    let expected = format!(r#"42["test","{}"]"#, "a".repeat(512));
    assert_eq!(timeout_rcv!(&mut rx1, 500), expected);
    assert_eq!(timeout_rcv!(&mut rx2, 500), expected);

    assert_eq!(h1.push_count(), 0, "server 1 must not push attachments");
    assert_eq!(h2.push_count(), 0, "server 2 must not push attachments");
    assert_eq!(h1.fetch_count(), 0, "server 1 must not fetch attachments");
    assert_eq!(h2.fetch_count(), 0, "server 2 must not fetch attachments");
}

/// Golden path: a large broadcast from server 1 is stored in the attachment table, server 2
/// fetches it and delivers it to its socket. Payload must round-trip byte-for-byte.
#[tokio::test]
async fn broadcast_above_threshold_uses_attachment() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();

    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let msg = "y".repeat(4096);
        socket.broadcast().emit("test", &msg).await.unwrap();
    }

    let ([io1, io2], [h1, h2]) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());

    io1.ns("/", handler).await.unwrap();
    io2.ns("/", handler).await.unwrap();

    let ((_tx1, mut rx1), (_tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    let expected = format!(r#"42["test","{}"]"#, "y".repeat(4096));
    assert_eq!(timeout_rcv!(&mut rx1, 500), expected);
    assert_eq!(timeout_rcv!(&mut rx2, 500), expected);

    assert!(h1.push_count() >= 1, "server 1 should push an attachment");
    assert!(h2.push_count() >= 1, "server 2 should push an attachment");
    assert!(
        h1.fetch_count() >= 1,
        "server 1 should fetch server 2's attachment"
    );
    assert!(
        h2.fetch_count() >= 1,
        "server 2 should fetch server 1's attachment"
    );
}

/// A large NOTIFY the sender self-delivers (PG delivers NOTIFY to the emitter too) must be
/// filtered by `is_loopback` BEFORE hitting the attachment table — otherwise each server
/// does a pointless DB round-trip for every large message it itself produced.
#[tokio::test]
async fn request_with_attachment_loopback_is_filtered() {
    async fn handler<A: Adapter>(socket: SocketRef<A>) {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let msg = "z".repeat(2048);
        socket.broadcast().emit("test", &msg).await.unwrap();
    }

    let ([io1], [h1]) = fixture::spawn_servers_with_handles::<1>(low_threshold_config());
    io1.ns("/", handler).await.unwrap();

    let (_tx1, mut rx1) = io1.new_dummy_sock("/", ()).await;
    timeout_rcv!(&mut rx1); // Connect "/" packet

    // Give the handle_ev_stream loop time to consume the self-delivered NOTIFY.
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(h1.push_count() >= 1, "sender must push the attachment");
    assert_eq!(
        h1.fetch_count(),
        0,
        "sender must not fetch its own attachment (loopback filter)"
    );
}

/// Large ack responses must round-trip through the attachment table too. This exercises
/// `send_res` → `ResponsePayload::Attachment` → `resolve_resp_payload` on the ack stream.
#[tokio::test]
async fn ack_stream_with_large_responses() {
    use futures_util::StreamExt;

    async fn emitter<A: Adapter>(socket: SocketRef<A>) {
        tokio::time::sleep(Duration::from_millis(5)).await;
        socket
            .broadcast()
            .emit_with_ack::<_, String>("test", "ping")
            .await
            .unwrap()
            .for_each(|(_, res)| {
                socket.emit("ack_res", &res.unwrap()).unwrap();
                async move {}
            })
            .await;
    }

    let ([io1, io2], [h1, h2]) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());

    io1.ns("/", emitter).await.unwrap();
    io2.ns("/", async || ()).await.unwrap();

    let ((_tx1, mut rx1), (tx2, mut rx2)) =
        tokio::join!(io1.new_dummy_sock("/", ()), io2.new_dummy_sock("/", ()));

    timeout_rcv!(&mut rx1); // Connect "/" packet
    timeout_rcv!(&mut rx2); // Connect "/" packet

    // Server 2's socket sees the emit-with-ack request and answers with a large payload.
    timeout_rcv!(&mut rx2, 100);
    let big_ack = filler(4096);
    let packet_res = format!(r#"431["{big_ack}"]"#).try_into().unwrap();
    tx2.try_send(packet_res).unwrap();

    // Server 1's socket receives the forwarded ack payload.
    let received = timeout_rcv!(&mut rx1, 500);
    assert!(
        received.contains(&big_ack),
        "ack payload was truncated or lost: {received}"
    );

    assert!(
        h2.push_count() >= 1,
        "server 2 should push an ack attachment"
    );
    assert!(
        h1.fetch_count() >= 1,
        "server 1 should fetch the ack attachment"
    );
}

/// Cross-server `fetch_sockets` response payloads become large when many sockets with many
/// rooms live on the remote. Exercises the read path through `get_res`.
#[tokio::test]
async fn fetch_sockets_response_uses_attachment() {
    let ([io1, io2], [_h1, h2]) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());

    let handler = |rooms: Vec<String>| async move |socket: SocketRef<_>| socket.join(rooms);

    let many_rooms: Vec<String> = (0..200).map(|i| format!("room-{i}")).collect();
    io1.ns("/", async || ()).await.unwrap();
    io2.ns("/", handler(many_rooms)).await.unwrap();

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;
    timeout_rcv!(&mut rx1);
    timeout_rcv!(&mut rx2);

    let sockets = io1.fetch_sockets().await.unwrap();
    assert_eq!(sockets.len(), 2);

    assert!(
        h2.push_count() >= 1,
        "server 2 should push its FetchSockets response via the attachment table"
    );
}

/// `rooms()` response path: many rooms reported by the remote exceed the threshold.
#[tokio::test]
async fn rooms_response_uses_attachment() {
    let ([io1, io2], [_h1, h2]) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());

    let handler = |rooms: Vec<String>| async move |socket: SocketRef<_>| socket.join(rooms);

    let many_rooms: Vec<String> = (0..200).map(|i| format!("room-{i}")).collect();
    io1.ns("/", async || ()).await.unwrap();
    io2.ns("/", handler(many_rooms.clone())).await.unwrap();

    let (_, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_, mut rx2) = io2.new_dummy_sock("/", ()).await;
    timeout_rcv!(&mut rx1);
    timeout_rcv!(&mut rx2);

    let rooms = io1.rooms().await.unwrap();
    assert!(rooms.len() >= many_rooms.len());

    assert!(
        h2.push_count() >= 1,
        "server 2 should push its AllRooms response via the attachment table"
    );
}

/// A corrupted attachment (decode fails) must not derail the pipeline: the poisoned packet
/// is dropped with a warning, and the next well-formed request is still processed.
#[tokio::test]
async fn attachment_resolution_failure_drops_packet() {
    async fn on_test<A: Adapter>(socket: SocketRef<A>) {
        let _ = socket;
    }

    let ([io1, io2], [h1, h2]) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());
    io1.ns("/", on_test).await.unwrap();
    io2.ns("/", on_test).await.unwrap();

    let (_tx1, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_tx2, mut rx2) = io2.new_dummy_sock("/", ()).await;
    timeout_rcv!(&mut rx1);
    timeout_rcv!(&mut rx2);

    // Arm server 2 to fail the next get_attachment call (the upcoming large broadcast).
    h2.fail_once();
    let msg1 = "a".repeat(4096);
    io1.emit("test", &msg1).await.unwrap();

    // Server 2's socket should not receive the dropped packet within a reasonable window.
    timeout_rcv_err!(&mut rx2);

    // Next broadcast must still get through — the pipeline is not poisoned.
    let msg2 = "b".repeat(4096);
    io1.emit("test", &msg2).await.unwrap();
    let expected = format!(r#"42["test","{}"]"#, "b".repeat(4096));
    assert_eq!(timeout_rcv!(&mut rx2, 500), expected);

    assert!(h1.push_count() >= 2, "two large broadcasts were emitted");
    assert!(
        h2.fetch_count() >= 2,
        "server 2 should have attempted to fetch both"
    );
}

/// Many large requests fired back-to-back must all reach the target. Catches regressions in
/// the buffered pipeline (e.g. stream closing early, head-of-line deadlocks).
#[tokio::test]
async fn concurrent_large_requests_reach_target() {
    const N: usize = 20;

    let ([io1, io2], _) = fixture::spawn_servers_with_handles::<2>(low_threshold_config());
    io1.ns("/", async || ()).await.unwrap();
    io2.ns("/", async || ()).await.unwrap();

    let (_tx1, mut rx1) = io1.new_dummy_sock("/", ()).await;
    let (_tx2, mut rx2) = io2.new_dummy_sock("/", ()).await;
    timeout_rcv!(&mut rx1);
    timeout_rcv!(&mut rx2);

    for i in 0..N {
        let msg = format!("{i}:{}", "q".repeat(4096));
        io1.emit("test", &msg).await.unwrap();
    }

    for _ in 0..N {
        let _ = timeout_rcv!(&mut rx2, 500);
    }
    timeout_rcv_err!(&mut rx2);
}
