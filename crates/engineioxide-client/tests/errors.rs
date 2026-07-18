//! Transport error handling tests.
//!
//! Reference behavior (engine.io-client, protocol v4):
//! * A non-2xx HTTP status on a polling GET/POST is a transport error: the
//!   client emits `error` then closes with reason "transport error". Mapped
//!   to this API: the stream (or the sink call) yields an `Err`, then the
//!   stream terminates. It must never panic, and never keep polling.
//! * A payload that cannot be decoded produces `Error("server error")` and
//!   the same error-then-close sequence.
//! * `noop` packets are ignored (their only purpose is releasing a held
//!   poll during upgrades).
//! * The client never retries or reconnects on its own.

use std::time::Duration;

use engineioxide_client::EioEvent;
use engineioxide_core::{Packet, TransportType};
use futures_util::{SinkExt, StreamExt};

mod mock;

/// Drive the stream expecting exactly one error and then termination.
async fn expect_error_then_termination(client: &mut engineioxide_client::Client<mock::MockSvc>) {
    let first = mock::within("stream error", client.next()).await;
    assert!(
        matches!(first, Some(Err(_))),
        "a transport error must surface as a stream error, got: {first:?}"
    );
    assert!(
        mock::within("stream termination after the error", client.next())
            .await
            .is_none(),
        "the stream must terminate after a transport error"
    );
}

/// A 500 on a mid-session poll must surface an error and close the session.
#[tokio::test]
async fn polling_get_http_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::join!(expect_error_then_termination(&mut client), async {
        server.next_http().await.respond(500, "")
    },);
    server
        .assert_no_call(Duration::from_millis(300), "the session is closed")
        .await;
}

/// A 400 `{"code":1,"message":"Session ID unknown"}` (e.g. the server
/// restarted and lost the session) must surface an error and close: this is
/// what a higher layer observes to decide to reconnect.
#[tokio::test]
async fn polling_get_session_unknown_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::join!(expect_error_then_termination(&mut client), async {
        server
            .next_http()
            .await
            .respond(400, "{\"code\":1,\"message\":\"Session ID unknown\"}")
    },);
    server
        .assert_no_call(Duration::from_millis(300), "the session is closed")
        .await;
}

/// A network-level failure on a mid-session poll must surface an error and
/// close the session.
#[tokio::test]
async fn polling_get_network_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::join!(expect_error_then_termination(&mut client), async {
        server.next_http().await.fail("connection reset")
    },);
    server
        .assert_no_call(Duration::from_millis(300), "the session is closed")
        .await;
}

/// An HTTP error on a POST write must surface as a sink error (reference:
/// "xhr post error" → close "transport error") — not a panic.
#[tokio::test]
async fn polling_post_http_error_surfaces() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (res, _) = tokio::join!(
        mock::within("send", client.send(EioEvent::Message("hello".into()))),
        async {
            let post = server.next_post_parking_get().await;
            post.respond(413, "");
        },
    );
    assert!(
        res.is_err(),
        "an HTTP error on a write must surface as a sink error"
    );
}

/// An undecodable poll payload must surface an error and close the session
/// (reference: `Error("server error")`, close "transport error").
#[tokio::test]
async fn polling_parse_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    tokio::join!(expect_error_then_termination(&mut client), async {
        server.next_http().await.respond(200, "garbage!")
    },);
    server
        .assert_no_call(Duration::from_millis(300), "the session is closed")
        .await;
}

/// An invalid packet received over websocket must surface an error and
/// close the session.
#[tokio::test]
async fn ws_parse_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, _server, ws) = mock::connect_ws(&open).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    ws.send_text("garbage!");

    let first = mock::within("stream error", client.next()).await;
    assert!(
        matches!(first, Some(Err(_))),
        "an invalid packet must surface as a stream error, got: {first:?}"
    );
    // `ws` stays alive: the termination must come from the client itself.
    assert!(
        mock::within("stream termination after the error", client.next())
            .await
            .is_none(),
        "the stream must terminate after a parse error"
    );
    drop(ws);
}

/// `noop` packets must be ignored: no user-visible event, no error, and the
/// poll loop keeps running.
#[tokio::test]
async fn noop_packet_is_ignored_polling() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        mock::within("connect event", client.next())
            .await
            .unwrap()
            .unwrap(),
        EioEvent::Connect(open.sid)
    );

    let (event, _) = tokio::join!(
        mock::within("message after the noop", client.next()),
        async {
            server.next_http().await.respond_packets([Packet::Noop]);
            // The client must keep polling after the noop...
            let poll = server.next_http().await;
            // ...and the noop must not have surfaced anything.
            poll.respond_packets([Packet::Message("after-noop".into())]);
        },
    );
    assert_eq!(
        event.unwrap().unwrap(),
        EioEvent::Message("after-noop".into()),
        "the noop must be skipped silently"
    );
}
