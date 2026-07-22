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

use std::{assert_matches, time::Duration};

use engineioxide_client::{
    ClientError, EioEvent,
    transport::{
        polling::{PollingError, ProtocolError},
        ws::WsError,
    },
};
use engineioxide_core::{Packet, PacketParseError, TransportType};
use futures_util::SinkExt;
use http::StatusCode;

use crate::{
    helpers::{ClientTestExt, FutureTestExt},
    mock,
};

/// A 500 on a mid-session poll must surface an error and close the session.
#[tokio::test]
async fn polling_get_http_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    tokio::join!(async { server.next_http().await.respond(500, "") }, async {
        let err = client.next_err().timeout().await;
        assert_matches!(
            err,
            ClientError::Polling(PollingError::Protocol(ProtocolError::ServerError {
                status: StatusCode::INTERNAL_SERVER_ERROR
            }))
        );
        client.next_close().timeout().await;
    });

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
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    tokio::join!(
        async {
            server
                .next_http()
                .await
                .respond(400, "{\"code\":1,\"message\":\"Session ID unknown\"}")
        },
        async {
            let err = client.next_err().timeout().await;
            assert_matches!(
                err,
                ClientError::Polling(PollingError::Protocol(ProtocolError::UnknownSessionID))
            );
            client.next_close().timeout().await;
        }
    );
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
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    tokio::join!(
        async { server.next_http().await.fail("connection reset") },
        async {
            let err = client.next_err().timeout().await;
            assert_matches!(err, ClientError::Polling(PollingError::Http(_)));
            client.next_close().timeout().await;
        }
    );
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
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    let (_, res) = tokio::join!(
        async { server.next_post_parking_get().await.respond(413, "") },
        async {
            client
                .send(EioEvent::Message("hello".into()))
                .timeout()
                .await
        },
    );
    assert_matches!(
        res,
        Err(ClientError::Polling(PollingError::Protocol(
            ProtocolError::InvalidRequest {
                status: StatusCode::PAYLOAD_TOO_LARGE
            }
        )))
    );

    client.next_close().timeout().await;
}

/// An undecodable poll payload must surface an error and close the session
/// (reference: `Error("server error")`, close "transport error").
#[tokio::test]
async fn polling_parse_error_surfaces_and_closes() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    tokio::join!(
        async { server.next_http().await.respond(200, "garbage!") },
        async {
            let err = client.next_err().timeout().await;
            assert_matches!(err, ClientError::Polling(PollingError::Packet(_)));
            client.next_close().timeout().await;
        }
    );
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
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    ws.send_text("garbage!");

    let err = client.next_err().timeout().await;
    assert_matches!(err, ClientError::Websocket(WsError::Packet(_)));

    client.next_close().timeout().await;
}

/// `noop` packets must be ignored: no user-visible event, no error, and the
/// poll loop keeps running.
#[tokio::test]
async fn noop_packet_is_ignored_polling() {
    let open = mock::open_packet_no_upgrade();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;
    assert_eq!(
        client.next_ok().timeout().await,
        EioEvent::Connect(open.sid)
    );

    let (event, _) = tokio::join!(client.next_ok().timeout(), async {
        server.next_http().await.respond_packets([Packet::Noop]);
        // The client must keep polling after the noop...
        let poll = server.next_http().await;
        // ...and the noop must not have surfaced anything.
        poll.respond_packets([Packet::Message("after-noop".into())]);
    },);

    assert_eq!(
        event,
        EioEvent::Message("after-noop".into()),
        "the noop must be skipped silently"
    );
}
