//! Heartbeat mechanism tests.
//!
//! In the engine.io v4 protocol the *server* drives the heartbeat: every
//! `ping_interval` it sends a `Packet::Ping` and expects a `Packet::Pong` back
//! within `ping_timeout`, otherwise it closes the socket with
//! [`DisconnectReason::HeartbeatTimeout`].
//!
//! The pong is emitted transparently by `Client::poll_next`: a `Ping` is
//! intercepted, a `Pong` is sent and the `Ping` is never surfaced to the user.

use std::time::Duration;

use engineioxide::config::EngineIoConfig;
use engineioxide_client::{Client, EioEvent};
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service_with_config};

mod fixture;

const PING_INTERVAL: Duration = Duration::from_millis(100);
const PING_TIMEOUT: Duration = Duration::from_millis(100);

fn config() -> EngineIoConfig {
    EngineIoConfig::builder()
        .ping_interval(PING_INTERVAL)
        .ping_timeout(PING_TIMEOUT)
        .build()
}
/// A [`Client`] that is continuously polled must auto-respond to the server's
/// `Ping`s with `Pong`s, keeping the connection alive across several ping
/// cycles. The connection must also still be usable afterwards.
#[tokio::test]
async fn heartbeat_keeps_connection_alive() {
    let (svc, mut rx) = service_with_config(config());

    let mut client = Client::connect_polling(svc).await.unwrap();
    let sid = client.sid;
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));

    // Drive the client for several ping cycles. Any server event during this
    // window can only be a disconnect (we send nothing), which would mean the
    // heartbeat failed.
    let window = PING_INTERVAL * 5;
    let deadline = tokio::time::sleep(window);
    tokio::pin!(deadline);
    tokio::select! {
        _ = &mut deadline => (),
        ev = rx.recv() => panic!("unexpected server event during heartbeat: {ev:?}"),
        // Polling the stream is what lets the client receive `Ping`s and
        // emit `Pong`s. `Ping` is consumed internally and never yielded,
        // so this branch effectively never resolves.
        packet = client.next() => match packet {
            Some(Ok(p)) => panic!("unexpected packet during heartbeat: {p:?}"),
            Some(Err(e)) => panic!("client stream error during heartbeat: {e:?}"),
            None => panic!("client stream ended unexpectedly"),
        },
    }

    // The connection survived several ping cycles: prove it is still healthy
    // by round-tripping a message (the handler echoes it back).
    client.send(EioEvent::Message("hb".into())).await.unwrap();
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "hb".into()),
        "server should still receive messages after the heartbeat window",
    );
    match client.next().await {
        Some(Ok(EioEvent::Message(msg))) => {
            assert_eq!(msg, "hb")
        }
        // Ignore any other packet (should not happen, `Ping` is internal).
        Some(Ok(p)) => panic!("unexpected packet: {p:?}"),
        Some(Err(e)) => panic!("client stream error: {e:?}"),
        None => panic!("client stream ended before echo"),
    }
}

/// A [`Client`] that is continuously polled must auto-respond to the server's
/// `Ping`s with `Pong`s, keeping the connection alive across several ping
/// cycles. The connection must also still be usable afterwards.
#[tokio::test]
async fn heartbeat_keeps_connection_alive_websocket() {
    let (svc, mut rx) = service_with_config(config());

    let mut client = Client::connect_ws(svc).await.unwrap();
    let sid = client.sid;
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));

    // Drive the client for several ping cycles. Any server event during this
    // window can only be a disconnect (we send nothing), which would mean the
    // heartbeat failed.
    let window = PING_INTERVAL * 5;
    let deadline = tokio::time::sleep(window);
    tokio::pin!(deadline);
    tokio::select! {
        _ = &mut deadline => (),
        ev = rx.recv() => panic!("unexpected server event during heartbeat: {ev:?}"),
        // Polling the stream is what lets the client receive `Ping`s and
        // emit `Pong`s. `Ping` is consumed internally and never yielded,
        // so this branch effectively never resolves.
        packet = client.next() => match packet {
            Some(Ok(p)) => panic!("unexpected packet during heartbeat: {p:?}"),
            Some(Err(e)) => panic!("client stream error during heartbeat: {e:?}"),
            None => panic!("client stream ended unexpectedly"),
        },
    }

    // The connection survived several ping cycles: prove it is still healthy
    // by round-tripping a message (the handler echoes it back).
    client.send(EioEvent::Message("hb".into())).await.unwrap();
    assert_eq!(
        rx.recv().await.unwrap(),
        Event::Message(sid, "hb".into()),
        "server should still receive messages after the heartbeat window",
    );
    match client.next().await {
        Some(Ok(EioEvent::Message(msg))) => {
            assert_eq!(msg, "hb")
        }
        // Ignore any other packet (should not happen, `Ping` is internal).
        Some(Ok(p)) => panic!("unexpected packet: {p:?}"),
        Some(Err(e)) => panic!("client stream error: {e:?}"),
        None => panic!("client stream ended before echo"),
    }
}
