//! Closing behavior tests.
//!
//! Reference behavior (engine.io-client, protocol v4):
//! * Server-initiated close: a `1` (close) packet ends the session — the
//!   reference client fires `close` with reason "transport close". Mapped to
//!   this API: the stream yields [`EioEvent::Disconnect`], then terminates.
//! * Client-initiated close: buffered packets are flushed first, then the
//!   polling transport POSTs a `1` (close) packet, while the websocket
//!   transport simply closes the connection (no close packet). The reference
//!   reason is "forced close".
//! * Packets submitted after a close are never delivered (the reference
//!   client silently discards them).
//! * An abruptly closed websocket is a *clean* close ("transport close"),
//!   not an error.

use std::time::Duration;

use engineioxide::{DisconnectReason, TransportType};
use engineioxide_client::{Client, EioEvent, EngineIoClientConfig};
use engineioxide_core::Packet;
use futures_util::{SinkExt, StreamExt};

use crate::fixture::{Event, service, service_with_registry};

mod fixture;
mod mock;

/// A `1` (close) packet received on a poll ends the session: `Disconnect`
/// then end-of-stream, and the client stops issuing requests.
#[tokio::test]
async fn server_close_packet_polling() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (disconnect, _) = tokio::join!(mock::within("disconnect event", client.next()), async {
        server.next_http().await.respond_packets([Packet::Close])
    },);
    assert_eq!(disconnect.unwrap().unwrap(), EioEvent::Disconnect);
    assert_eq!(
        mock::within("end of stream", client.next())
            .await
            .map(|r| r.ok()),
        None,
        "the stream must terminate after the server closed the session"
    );
}

/// A `1` (close) packet received on the websocket ends the session the same
/// way.
#[tokio::test]
async fn server_close_packet_ws() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    ws.send_packet(Packet::Close);
    assert_eq!(
        mock::within("disconnect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Disconnect
    );
    assert_eq!(
        mock::within("end of stream", client.next())
            .await
            .map(|r| r.ok()),
        None,
        "the stream must terminate after the server closed the session"
    );
}

/// Server-initiated close against the real server, polling transport.
#[tokio::test]
async fn server_close_real_server_polling() {
    let (svc, mut rx, registry) = service_with_registry(Default::default());
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

    registry.lock().unwrap()[&sid].close(DisconnectReason::TransportClose);

    assert_eq!(
        fixture::within("disconnect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Disconnect
    );
    assert!(
        fixture::within("end of stream", client.next())
            .await
            .is_none(),
        "the stream must terminate after the server closed the session"
    );
}

/// Server-initiated close against the real server, websocket transport.
#[tokio::test]
async fn server_close_real_server_ws() {
    let (svc, mut rx, registry) = service_with_registry(Default::default());
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

    registry.lock().unwrap()[&sid].close(DisconnectReason::TransportClose);

    assert_eq!(
        fixture::within("disconnect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Disconnect
    );
    assert!(
        fixture::within("end of stream", client.next())
            .await
            .is_none(),
        "the stream must terminate after the server closed the session"
    );
}

/// Closing a polling client must send a `1` (close) packet so the server
/// learns about the disconnection immediately (reference `Polling.doClose`
/// writes a close packet).
#[tokio::test]
async fn client_close_polling_sends_close_packet() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::join!(
        async {
            mock::within("client close", client.close())
                .await
                .expect("close must succeed");
        },
        async {
            let post = server.next_post_parking_get().await;
            assert_eq!(
                post.packets(),
                vec![Packet::Close],
                "closing a polling client must POST a close packet"
            );
            post.respond_ok();
        },
    );
}

/// Closing a websocket client closes the connection without sending a `1`
/// packet (reference `WS.doClose` just closes the socket).
#[tokio::test]
async fn client_close_ws_closes_the_connection() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, mut ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    mock::within("client close", client.close())
        .await
        .expect("close must succeed");

    // The server observes the websocket closing, with no engine.io close
    // packet beforehand.
    match mock::within("ws close", ws.recv()).await {
        Some(engineioxide_client::transport::ws::WsMessage::Close) | None => (),
        Some(engineioxide_client::transport::ws::WsMessage::Text(t)) => {
            assert_ne!(&*t, "1", "no close packet is sent over websocket");
            panic!("unexpected frame while closing: {t:?}");
        }
        Some(engineioxide_client::transport::ws::WsMessage::Binary(_)) => {
            panic!("unexpected binary frame while closing")
        }
    }
}

/// After a client-side close the real server must observe the disconnection
/// promptly (i.e. via the close packet, not a heartbeat timeout — the
/// default ping budget is 45s while the test deadline is 5s), polling
/// transport.
#[tokio::test]
async fn client_close_notifies_server_polling() {
    let (svc, mut rx) = service();
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

    fixture::within("client close", client.close())
        .await
        .expect("close must succeed");

    assert_eq!(
        fixture::within("server disconnect event", rx.recv())
            .await
            .unwrap(),
        Event::Disconnect(sid, DisconnectReason::TransportClose),
        "the server must observe a graceful close"
    );
}

/// Same requirement over websocket: closing the connection is enough for
/// the server to observe a graceful disconnection.
#[tokio::test]
async fn client_close_notifies_server_ws() {
    let (svc, mut rx) = service();
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

    fixture::within("client close", client.close())
        .await
        .expect("close must succeed");

    assert_eq!(
        fixture::within("server disconnect event", rx.recv())
            .await
            .unwrap(),
        Event::Disconnect(sid, DisconnectReason::TransportClose),
        "the server must observe a graceful close"
    );
}

/// Packets buffered before a close must be flushed before the transport
/// closes (reference client waits for `drain` before closing), polling
/// transport.
#[tokio::test]
async fn close_flushes_buffered_packets_polling() {
    let (svc, mut rx) = service();
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

    // feed() queues without flushing; close() must flush then close.
    client.feed(EioEvent::Message("one".into())).await.unwrap();
    client.feed(EioEvent::Message("two".into())).await.unwrap();
    fixture::within("client close", client.close())
        .await
        .expect("close must succeed");

    assert_eq!(
        fixture::within("first buffered message", rx.recv())
            .await
            .unwrap(),
        Event::Message(sid, "one".into())
    );
    assert_eq!(
        fixture::within("second buffered message", rx.recv())
            .await
            .unwrap(),
        Event::Message(sid, "two".into())
    );
    assert_eq!(
        fixture::within("server disconnect event", rx.recv())
            .await
            .unwrap(),
        Event::Disconnect(sid, DisconnectReason::TransportClose),
    );
}

/// Same flush-before-close requirement over websocket.
#[tokio::test]
async fn close_flushes_buffered_packets_ws() {
    let (svc, mut rx) = service();
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

    client.feed(EioEvent::Message("one".into())).await.unwrap();
    client.feed(EioEvent::Message("two".into())).await.unwrap();
    fixture::within("client close", client.close())
        .await
        .expect("close must succeed");

    assert_eq!(
        fixture::within("first buffered message", rx.recv())
            .await
            .unwrap(),
        Event::Message(sid, "one".into())
    );
    assert_eq!(
        fixture::within("second buffered message", rx.recv())
            .await
            .unwrap(),
        Event::Message(sid, "two".into())
    );
    assert_eq!(
        fixture::within("server disconnect event", rx.recv())
            .await
            .unwrap(),
        Event::Disconnect(sid, DisconnectReason::TransportClose),
    );
}

/// Packets submitted after a local close must never reach the server (the
/// reference client silently discards them; a sink error is also
/// acceptable — but panicking or delivering is not).
#[tokio::test]
async fn send_after_close_is_not_delivered() {
    let (svc, mut rx) = service();
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

    fixture::within("client close", client.close())
        .await
        .expect("close must succeed");

    // Whether this returns Ok (discarded) or Err (sink closed) is API
    // flavor; what matters is that nothing is delivered and nothing panics.
    let _ = fixture::within(
        "send after close",
        client.send(EioEvent::Message("late".into())),
    )
    .await;

    if let Ok(Some(Event::Message(_, msg))) =
        tokio::time::timeout(Duration::from_millis(300), rx.recv()).await
    {
        panic!("a message sent after close must not be delivered: {msg:?}")
    }
}

/// Sending after the *server* closed the session must surface a sink error
/// (the session is gone), not panic.
#[tokio::test]
async fn send_after_server_close_is_an_error() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (disconnect, _) = tokio::join!(mock::within("disconnect event", client.next()), async {
        server.next_http().await.respond_packets([Packet::Close])
    },);
    assert_eq!(disconnect.unwrap().unwrap(), EioEvent::Disconnect);
    assert!(
        mock::within("end of stream", client.next()).await.is_none(),
        "the stream must terminate after the server closed the session"
    );

    let res = mock::within(
        "send after server close",
        client.send(EioEvent::Message("late".into())),
    )
    .await;
    assert!(
        res.is_err(),
        "sending on a closed session must surface an error"
    );
}

/// An abrupt websocket termination is a clean close for the reference
/// client ("transport close"), not an error: the stream must terminate
/// without yielding one.
#[tokio::test]
async fn abrupt_ws_termination_ends_the_stream() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    ws.close();

    mock::within("stream termination", async {
        loop {
            match client.next().await {
                // Surfacing the disconnection as an event first is fine.
                Some(Ok(EioEvent::Disconnect)) => continue,
                Some(Ok(ev)) => panic!("unexpected event on abrupt close: {ev:?}"),
                Some(Err(e)) => panic!("an abrupt close is not an error: {e}"),
                None => break,
            }
        }
    })
    .await;
}

/// A websocket-level error must surface as a stream error, then the stream
/// must terminate (reference client: `error` event, then close with reason
/// "transport error").
#[tokio::test]
async fn ws_error_surfaces_then_stream_terminates() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    ws.send_error("connection reset by peer");

    let err = mock::within("stream error", client.next()).await;
    assert!(
        matches!(err, Some(Err(_))),
        "the websocket error must surface: {err:?}"
    );
    // Keep `ws` alive: termination must come from the client closing itself
    // after the error, not from the mock dropping the connection.
    assert!(
        mock::within("stream termination after error", client.next())
            .await
            .is_none(),
        "the stream must terminate after a transport error"
    );
    drop(ws);
}
