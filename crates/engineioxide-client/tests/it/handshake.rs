//! Handshake tests.
//!
//! Reference behavior (engine.io-client, protocol v4):
//! * The handshake request is a `GET` carrying `EIO=4` and `transport=...`
//!   query parameters, and no `sid` (the session does not exist yet).
//!   User-provided query parameters must be preserved.
//! * The server answers with an `open` packet:
//!   `0{"sid":...,"upgrades":[...],"pingInterval":...,"pingTimeout":...,"maxPayload":...}`.
//! * The `sid` is then attached to every subsequent request.
//! * Anything else — a non-open packet, malformed JSON, an HTTP error status,
//!   a network failure, a closed websocket — must surface as a connection
//!   error.

use std::assert_matches;

use engineioxide_client::{Client, ConnectError, EioEvent, EngineIoClientConfig};
use engineioxide_core::{Packet, TransportType};
use http::Method;

use crate::mock::{
    self,
    helpers::{ClientTestExt, FutureTestExt},
};

/// The polling handshake must be a `GET` on the configured path with
/// `EIO=4&transport=polling` and no `sid`.
#[tokio::test]
async fn polling_handshake_request_format() {
    let (svc, mut server) = mock::mock();
    let config = EngineIoClientConfig::builder()
        .uri("http://example.com/engine.io")
        .transports([TransportType::Polling])
        .build();

    let (client, _) = tokio::join!(Client::connect(svc, config).timeout(), async {
        let call = server.next_http().await;
        assert_eq!(call.method, Method::GET);
        assert_eq!(call.uri.path(), "/engine.io");
        assert_eq!(call.query("EIO"), Some("4"));
        assert_eq!(call.query("transport"), Some("polling"));
        assert_eq!(call.query("sid"), None, "no sid before the session exists");
        assert!(call.body.is_empty(), "the handshake GET has no body");
        call.respond_open(&mock::open_packet());
    },);
    let client = client.expect("handshake should succeed");
    assert_eq!(client.transport(), TransportType::Polling);
}

/// The websocket handshake must carry `EIO=4&transport=websocket` and no
/// `sid` when connecting directly (no polling session to upgrade).
#[tokio::test]
async fn ws_handshake_request_format() {
    let (svc, mut server) = mock::mock();
    let config = EngineIoClientConfig::builder()
        .uri("http://example.com/engine.io")
        .transports([TransportType::Websocket])
        .build();

    let (client, _ws) = tokio::join!(Client::connect(svc, config).timeout(), async {
        let call = server.next_ws().await;
        assert_eq!(call.req.uri().path(), "/engine.io");
        assert_eq!(call.query("EIO"), Some("4"));
        assert_eq!(call.query("transport"), Some("websocket"));
        assert_eq!(call.query("sid"), None, "no sid before the session exists");
        let ws = call.accept();
        ws.send_packet(Packet::Open(mock::open_packet()));
        ws
    },);
    let client = client.expect("handshake should succeed");
    assert_eq!(client.transport(), TransportType::Websocket);
}

/// Once the open packet is received, its `sid` must be exposed and attached
/// to every subsequent request.
#[tokio::test]
async fn handshake_sid_is_attached_to_subsequent_requests() {
    let open = mock::open_packet();
    let sid = open.sid.to_string();
    let (mut client, mut server) = mock::connect_polling(&open, [TransportType::Polling]).await;

    assert_eq!(client.sid(), open.sid);
    assert_eq!(client.next_ok().await, EioEvent::Connect(open.sid));

    let (event, _) = tokio::join!(client.next_ok().timeout(), async {
        let call = server.next_http().await;
        assert_eq!(call.method, Method::GET);
        assert_eq!(call.query("sid").map(str::to_owned), Some(sid));
        assert_eq!(call.query("EIO"), Some("4"));
        assert_eq!(call.query("transport"), Some("polling"));
        call.respond_packets([Packet::Message("hi".into())]);
    },);
    assert_eq!(event, EioEvent::Message("hi".into()));
}

/// Query parameters provided in the configured uri must be preserved
/// alongside the mandatory engine.io parameters.
#[tokio::test]
async fn handshake_preserves_custom_query_params() {
    let (svc, mut server) = mock::mock();
    let config = EngineIoClientConfig::builder()
        .uri("http://example.com/engine.io?token=s3cret")
        .transports([TransportType::Polling])
        .build();

    let (client, _) = tokio::join!(Client::connect(svc, config).timeout(), async {
        let call = server.next_http().await;
        assert_eq!(call.query("token"), Some("s3cret"));
        assert_eq!(call.query("EIO"), Some("4"));
        assert_eq!(call.query("transport"), Some("polling"));
        call.respond_open(&mock::open_packet());
    },);
    client.expect("handshake should succeed");
    //TODO: not finished
}

/// Run a polling connect against a scripted handshake answer and require it
/// to fail.
async fn polling_connect_must_fail(answer: impl FnOnce(mock::HttpCall)) {
    let (svc, mut server) = mock::mock();

    let (res, _) = tokio::join!(
        Client::connect(svc, [TransportType::Polling]).timeout(),
        async { answer(server.next_http().await) },
    );
    assert_matches!(res, Err(ConnectError::Client(_)));
}

/// Run a websocket connect against a scripted handshake answer and require
/// it to fail.
async fn ws_connect_must_fail(answer: impl FnOnce(mock::WsCall)) {
    let (svc, mut server) = mock::mock();
    let (res, _) = tokio::join!(
        Client::connect(svc, [TransportType::Websocket]).timeout(),
        async { answer(server.next_ws().await) },
    );
    assert_matches!(res, Err(ConnectError::Client(_)));
}

/// A first packet that is not an `open` packet is a protocol violation.
#[tokio::test]
async fn handshake_rejects_non_open_packet() {
    polling_connect_must_fail(|call| call.respond(200, "4hello")).await;
}

/// An open packet with a malformed JSON payload must be rejected.
#[tokio::test]
async fn handshake_rejects_malformed_open_packet() {
    polling_connect_must_fail(|call| call.respond(200, "0{\"sid\":")).await;
}

/// An HTTP error status on the handshake (e.g. the server refuses the
/// transport) must be surfaced as a connection error.
#[tokio::test]
async fn handshake_rejects_http_error_status() {
    polling_connect_must_fail(|call| {
        call.respond(400, "{\"code\":0,\"message\":\"Transport unknown\"}")
    })
    .await;
}

/// An empty 500 response must be surfaced as a connection error.
#[tokio::test]
async fn handshake_rejects_server_error_status() {
    polling_connect_must_fail(|call| call.respond(500, "")).await;
}

/// A network-level failure during the handshake must be surfaced.
#[tokio::test]
async fn handshake_network_error() {
    polling_connect_must_fail(|call| call.fail("connection refused")).await;
}

/// Over websocket, a first packet that is not an `open` packet is a protocol
/// violation.
#[tokio::test]
async fn ws_handshake_rejects_non_open_packet() {
    ws_connect_must_fail(|call| {
        let ws = call.accept();
        ws.send_text("4hello");
    })
    .await;
}

/// A websocket closed before sending the open packet must surface an error.
#[tokio::test]
async fn ws_handshake_rejects_immediate_close() {
    ws_connect_must_fail(|call| {
        let ws = call.accept();
        ws.close();
    })
    .await;
}

/// A refused websocket connection must surface an error.
#[tokio::test]
async fn ws_handshake_rejects_refused_connection() {
    ws_connect_must_fail(|call| call.reject("connection refused")).await;
}
