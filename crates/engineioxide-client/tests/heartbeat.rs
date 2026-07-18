//! Heartbeat mechanism tests.
//!
//! In the engine.io v4 protocol the *server* drives the heartbeat: every
//! `ping_interval` it sends a `Packet::Ping` and expects a `Packet::Pong` back
//! within `ping_timeout`, otherwise it closes the socket with
//! [`DisconnectReason::HeartbeatTimeout`].
//!
//! The pong is emitted transparently by `Client::poll_next`: a `Ping` is
//! intercepted, a `Pong` is sent and the `Ping` is never surfaced to the user.

use std::time::{Duration, Instant};

use engineioxide::{TransportType, config::EngineIoConfig};
use engineioxide_client::{Client, EioEvent, EngineIoClientConfig};
use engineioxide_core::{OpenPacket, Packet};
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service_with_config};

mod fixture;
mod mock;

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
async fn heartbeat_keeps_connection_alive_polling() {
    let (svc, mut rx) = service_with_config(config());
    let config = EngineIoClientConfig::builder()
        .transports([TransportType::Polling])
        .build();
    let mut client = Client::connect(svc, config).await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    assert_eq!(
        client.next().await.unwrap().unwrap(),
        EioEvent::Connect(sid)
    );

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

/// Wire-level pong check over websocket: a server `2` (ping) frame must be
/// answered with a `3` (pong) frame, and the ping must never surface as a
/// user-visible event.
#[tokio::test]
async fn ping_answered_with_pong_ws() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, mut ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::select! {
        // Polling the stream is what drives the ping/pong exchange; the ping
        // is internal so this branch must never resolve.
        ev = client.next() => panic!("the ping must not surface: {ev:?}"),
        _ = async {
            for _ in 0..3 {
                ws.send_packet(Packet::Ping);
                assert_eq!(ws.recv_packet().await, Packet::Pong);
            }
        } => (),
    }
}

/// Wire-level pong check over polling: a `2` (ping) packet received on a GET
/// must be answered with a `3` (pong) POST, and never surface as an event.
#[tokio::test]
async fn ping_answered_with_pong_polling() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::select! {
        ev = client.next() => panic!("the ping must not surface: {ev:?}"),
        _ = async {
            let poll = server.next_http().await;
            poll.respond_packets([Packet::Ping]);
            let post = server.next_post_parking_get().await;
            assert_eq!(post.packets(), vec![Packet::Pong], "the ping must be answered with a pong");
            post.respond_ok();
        } => (),
    }
}

/// Reference client behavior: after the handshake the client arms a liveness
/// timer of `pingInterval + pingTimeout`; if no ping arrives in that window
/// it closes itself (official close reason: "ping timeout").
///
/// Here the server never pings: the client must terminate its stream within
/// the budget instead of waiting forever.
#[tokio::test]
async fn ping_timeout_closes_the_socket_polling() {
    let open = OpenPacket {
        ping_interval: Duration::from_millis(100),
        ping_timeout: Duration::from_millis(100),
        ..mock::open_packet_no_upgrade()
    };
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    let started = Instant::now();
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let drive = async {
        loop {
            match client.next().await {
                Some(Ok(ev)) => panic!("no event expected from a silent server: {ev:?}"),
                Some(Err(_)) => (), // an error surfacing the timeout is acceptable
                None => break,      // stream terminated: the socket closed itself
            }
        }
    };
    // The server stays silent: polls are held forever, no ping is ever sent.
    let park = async {
        loop {
            match server.next_call().await {
                mock::ServerCall::Http(c) => c.park(),
                mock::ServerCall::Ws(c) => c.park(),
            }
        }
    };
    tokio::time::timeout(Duration::from_millis(1500), async {
        tokio::select! { _ = drive => (), _ = park => unreachable!() }
    })
    .await
    .expect(
        "client must close itself within pingInterval + pingTimeout when the server stops pinging",
    );

    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(150),
        "client closed before the pingInterval + pingTimeout budget: {elapsed:?}"
    );
}

/// Same liveness requirement over websocket.
#[tokio::test]
async fn ping_timeout_closes_the_socket_ws() {
    let open = OpenPacket {
        ping_interval: Duration::from_millis(100),
        ping_timeout: Duration::from_millis(100),
        ..mock::open_packet_no_upgrade()
    };
    // `ws` must stay alive: dropping it would close the stream and make the
    // test pass without exercising the client-side timer.
    let (mut client, _server, ws) = mock::connect_ws(&open).await;
    let started = Instant::now();
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::time::timeout(Duration::from_millis(1500), async {
        loop {
            match client.next().await {
                Some(Ok(ev)) => panic!("no event expected from a silent server: {ev:?}"),
                Some(Err(_)) => (),
                None => break,
            }
        }
    })
    .await
    .expect(
        "client must close itself within pingInterval + pingTimeout when the server stops pinging",
    );

    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(150),
        "client closed before the pingInterval + pingTimeout budget: {elapsed:?}"
    );
    drop(ws);
}

/// The liveness timer must be re-armed by every received ping: as long as
/// the server keeps pinging (even past the initial budget), the client stays
/// alive; once pings stop, it must close within the budget.
#[tokio::test]
async fn ping_timeout_is_reset_by_each_ping() {
    let open = OpenPacket {
        ping_interval: Duration::from_millis(150),
        ping_timeout: Duration::from_millis(100),
        ..mock::open_packet_no_upgrade()
    };
    let (mut client, _server, mut ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    // 4 pings every 120ms: t=480ms, well past the 250ms budget. The client
    // must still be alive because each ping re-arms the timer.
    tokio::select! {
        ev = client.next() => panic!("client must stay alive while pings keep coming: {ev:?}"),
        _ = async {
            for _ in 0..4 {
                tokio::time::sleep(Duration::from_millis(120)).await;
                ws.send_packet(Packet::Ping);
                assert_eq!(ws.recv_packet().await, Packet::Pong);
            }
        } => (),
    }

    // Now the server goes silent: the client must close within the budget.
    tokio::time::timeout(Duration::from_millis(1500), async {
        loop {
            match client.next().await {
                Some(Ok(ev)) => panic!("no event expected from a silent server: {ev:?}"),
                Some(Err(_)) => (),
                None => break,
            }
        }
    })
    .await
    .expect("client must close with a ping timeout once pings stop");
}

/// A [`Client`] that is continuously polled must auto-respond to the server's
/// `Ping`s with `Pong`s, keeping the connection alive across several ping
/// cycles. The connection must also still be usable afterwards.
#[tokio::test]
async fn heartbeat_keeps_connection_alive_websocket() {
    let (svc, mut rx) = service_with_config(config());

    let config = EngineIoClientConfig::builder()
        .transports([TransportType::Websocket])
        .build();

    let mut client = Client::connect(svc, config).await.unwrap();
    let sid = client.sid();
    assert_eq!(rx.recv().await.unwrap(), Event::Connect(sid));
    assert_eq!(
        client.next().await.unwrap().unwrap(),
        EioEvent::Connect(sid)
    );

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
