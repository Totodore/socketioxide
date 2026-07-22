//! A scripted mock engine.io server.
//!
//! Unlike the `fixture` module (which runs the real `engineioxide` server
//! behind the [`TestingFlavor`]), this harness hands every HTTP request and
//! every websocket connection straight to the test, which decides how to
//! answer (or not answer) each one. It is the "granular control" path used to
//! exercise edge cases the real server never produces: malformed handshakes,
//! dropped pings, failed upgrades, error status codes, abrupt closes...
//!
//! Test binaries using it must declare `mod mock;`.
#![allow(dead_code)]

use std::{
    convert::Infallible,
    fmt,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use engineioxide_client::{
    Client,
    transport::{WebSocket, ws::WsMessage},
};
use engineioxide_core::{OpenPacket, Packet, ProtocolVersion, Sid, TransportType};
use futures_core::{Stream, future::BoxFuture};
use futures_util::{FutureExt, Sink};
use http::{Method, Request, Response, Uri};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::service::Service as HyperSvc;
use tokio::sync::{mpsc, oneshot};
use tracing_subscriber::EnvFilter;

use crate::helpers::FutureTestExt;

/// The engine.io v4 payload record separator.
pub const SEP: char = '\x1e';

#[derive(Debug, Clone)]
pub struct MockError(pub String);
impl fmt::Display for MockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mock error: {}", self.0)
    }
}
impl std::error::Error for MockError {}

/// Create a connected `(client service, scripted server)` pair.
pub fn mock() -> (MockSvc, MockServer) {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .ok();
    let (tx, rx) = mpsc::unbounded_channel();
    (MockSvc { tx }, MockServer { rx })
}

/// The transport service handed to [`Client`]. Every request it receives is
/// forwarded to the paired [`MockServer`] for the test to answer.
#[derive(Debug, Clone)]
pub struct MockSvc {
    tx: mpsc::UnboundedSender<ServerCall>,
}

/// HTTP (polling) side: satisfies `PollingSvc`.
impl HyperSvc<Request<BoxBody<Bytes, Infallible>>> for MockSvc {
    type Response = Response<Full<Bytes>>;
    type Error = MockError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        let calls = self.tx.clone();
        // The request is surfaced to the test only once the client actually
        // polls the future, mirroring "the request was sent on the wire".
        async move {
            let (parts, body) = req.into_parts();
            let body = body.collect().await.unwrap().to_bytes();
            let (respond, rx) = oneshot::channel();
            let call = HttpCall {
                method: parts.method,
                uri: parts.uri,
                body,
                respond,
            };
            calls
                .send(ServerCall::Http(call))
                .map_err(|_| MockError("mock server dropped".into()))?;
            rx.await
                .map_err(|_| MockError("mock server dropped the response".into()))?
        }
        .boxed()
    }
}

/// Websocket side: satisfies `WsSvc`.
impl HyperSvc<Request<()>> for MockSvc {
    type Response = MockWs;
    type Error = MockError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: Request<()>) -> Self::Future {
        let calls = self.tx.clone();
        async move {
            let (respond, rx) = oneshot::channel();
            calls
                .send(ServerCall::Ws(WsCall { req, respond }))
                .map_err(|_| MockError("mock server dropped".into()))?;
            rx.await
                .map_err(|_| MockError("mock server dropped the connection".into()))?
        }
        .boxed()
    }
}

/// A channel-backed [`WebSocket`] implementation handed to the client when
/// the test accepts a websocket connection.
pub struct MockWs {
    to_server: mpsc::UnboundedSender<WsMessage>,
    from_server: mpsc::UnboundedReceiver<Result<WsMessage, MockError>>,
    closed: bool,
}

impl WebSocket for MockWs {
    type Error = MockError;
}

impl Stream for MockWs {
    type Item = Result<WsMessage, MockError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.from_server.poll_recv(cx)
    }
}

impl Sink<WsMessage> for MockWs {
    type Error = MockError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.closed {
            Poll::Ready(Err(MockError("websocket already closed".into())))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: WsMessage) -> Result<(), Self::Error> {
        if self.closed {
            return Err(MockError("websocket already closed".into()));
        }
        self.to_server
            .send(item)
            .map_err(|_| MockError("server closed the connection".into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        if !self.closed {
            // Emulate the websocket close handshake: the peer observes a
            // close frame.
            let _ = self.to_server.send(WsMessage::Close);
            self.closed = true;
        }
        Poll::Ready(Ok(()))
    }
}

/// A request the client issued, waiting for the test to decide its fate.
pub enum ServerCall {
    Http(HttpCall),
    Ws(WsCall),
}

impl ServerCall {
    pub fn describe(&self) -> String {
        match self {
            ServerCall::Http(c) => format!("http {} {}", c.method, c.uri),
            ServerCall::Ws(c) => format!("ws connect {}", c.req.uri()),
        }
    }
}

/// An in-flight HTTP request (polling GET or POST) with its collected body.
pub struct HttpCall {
    pub method: Method,
    pub uri: Uri,
    pub body: Bytes,
    respond: oneshot::Sender<Result<Response<Full<Bytes>>, MockError>>,
}

impl HttpCall {
    /// Value of a query parameter, if present.
    pub fn query(&self, key: &str) -> Option<&str> {
        query_param(&self.uri, key)
    }

    /// Decode the request body as engine.io packets (separated by `\x1e`).
    pub fn packets(&self) -> Vec<Packet> {
        decode_payload(&self.body)
    }

    pub fn respond(self, status: u16, body: impl Into<Bytes>) {
        let res = Response::builder()
            .status(status)
            .body(Full::new(body.into()))
            .unwrap();
        // The client may legitimately have dropped the request (e.g. a poll
        // abandoned on upgrade), so a send failure is not an error.
        let _ = self.respond.send(Ok(res));
    }

    /// Respond 200 with the given packets joined by the record separator.
    pub fn respond_packets(self, packets: impl IntoIterator<Item = Packet>) {
        let body = encode_payload(packets);
        self.respond(200, body);
    }

    /// Respond 200 with an open packet (handshake).
    pub fn respond_open(self, open: &OpenPacket) {
        self.respond_packets([Packet::Open(open.clone())]);
    }

    /// Respond 200 "ok", as the reference server does for POST writes.
    pub fn respond_ok(self) {
        self.respond(200, "ok");
    }

    /// Make the request fail with a transport-level (network) error.
    pub fn fail(self, msg: &str) {
        let _ = self.respond.send(Err(MockError(msg.into())));
    }

    /// Never answer: the request stays in flight forever (e.g. a held poll).
    pub fn park(self) {
        std::mem::forget(self.respond);
    }
}

/// An in-flight websocket connection request.
pub struct WsCall {
    pub req: Request<()>,
    respond: oneshot::Sender<Result<MockWs, MockError>>,
}

impl WsCall {
    pub fn query(&self, key: &str) -> Option<&str> {
        query_param(self.req.uri(), key)
    }

    /// Accept the connection and return the server-side handle.
    pub fn accept(self) -> ServerWs {
        let (to_client, from_server) = mpsc::unbounded_channel();
        let (to_server, from_client) = mpsc::unbounded_channel();
        let ws = MockWs {
            to_server,
            from_server,
            closed: false,
        };
        let _ = self.respond.send(Ok(ws));
        ServerWs {
            tx: to_client,
            rx: from_client,
        }
    }

    /// Refuse the connection with an error.
    pub fn reject(self, msg: &str) {
        let _ = self.respond.send(Err(MockError(msg.into())));
    }

    /// Never answer the connection request.
    pub fn park(self) {
        std::mem::forget(self.respond);
    }
}

/// Server-side handle over an accepted mock websocket.
pub struct ServerWs {
    tx: mpsc::UnboundedSender<Result<WsMessage, MockError>>,
    rx: mpsc::UnboundedReceiver<WsMessage>,
}

impl ServerWs {
    pub fn send_packet(&self, packet: Packet) {
        match packet {
            Packet::Binary(data) => self.send_message(WsMessage::Binary(data)),
            p => self.send_text(String::from(p)),
        }
    }

    pub fn send_text(&self, text: impl Into<String>) {
        self.send_message(WsMessage::Text(text.into().into()));
    }

    pub fn send_message(&self, msg: WsMessage) {
        self.tx.send(Ok(msg)).expect("client dropped the websocket");
    }

    /// Surface a websocket-level error to the client.
    pub fn send_error(&self, msg: &str) {
        self.tx
            .send(Err(MockError(msg.into())))
            .expect("client dropped the websocket");
    }

    /// Next raw message sent by the client, `None` once the client closed.
    pub async fn recv(&mut self) -> Option<WsMessage> {
        self.rx.recv().timeout().await
    }

    /// Next client message decoded as a packet (binary frames are messages).
    pub async fn recv_packet(&mut self) -> Packet {
        match self.recv().await {
            Some(WsMessage::Text(text)) => {
                Packet::parse(ProtocolVersion::V4, text).expect("client sent an invalid packet")
            }
            Some(WsMessage::Binary(data)) => Packet::Binary(data),
            Some(WsMessage::Close) => panic!("expected a packet, got a websocket close frame"),
            None => panic!("expected a packet, but the client closed the websocket"),
        }
    }

    /// Close the connection from the server side (client stream terminates).
    pub fn close(self) {}
}

/// Test-side handle receiving every request made by the client.
pub struct MockServer {
    rx: mpsc::UnboundedReceiver<ServerCall>,
}

impl MockServer {
    pub async fn next_call(&mut self) -> ServerCall {
        self.rx
            .recv()
            .timeout()
            .await
            .expect("client service dropped, no more requests will come")
    }

    pub async fn next_http(&mut self) -> HttpCall {
        match self.next_call().await {
            ServerCall::Http(call) => call,
            ServerCall::Ws(call) => {
                panic!("expected an http request, got a ws connect: {:?}", call.req)
            }
        }
    }

    pub async fn next_ws(&mut self) -> WsCall {
        match self.next_call().await {
            ServerCall::Ws(call) => call,
            ServerCall::Http(call) => panic!(
                "expected a ws connect, got an http request: {} {}",
                call.method, call.uri
            ),
        }
    }

    /// Wait for a ws connect, leaving any interleaved polling request parked
    /// (the reference client keeps a poll in flight while probing).
    pub async fn next_ws_parking_http(&mut self) -> WsCall {
        loop {
            match self.next_call().await {
                ServerCall::Ws(call) => return call,
                ServerCall::Http(call) => call.park(),
            }
        }
    }

    /// Wait for a POST write; polling GETs received meanwhile are parked.
    pub async fn next_post_parking_get(&mut self) -> HttpCall {
        loop {
            let call = self.next_http().await;
            if call.method == Method::POST {
                return call;
            }
            call.park()
        }
    }

    /// Assert the client stays silent (no http request, no ws connect) for
    /// the whole window.
    pub async fn assert_no_call(&mut self, window: Duration, what: &str) {
        match tokio::time::timeout(window, self.rx.recv()).await {
            Err(_) => (),   // silence: all good
            Ok(None) => (), // client service dropped: silent forever
            Ok(Some(call)) => panic!(
                "unexpected client request while {what}: {}",
                call.describe()
            ),
        }
    }
}

/// An open packet advertising a websocket upgrade.
pub fn open_packet() -> OpenPacket {
    OpenPacket {
        sid: Sid::new(),
        upgrades: [TransportType::Websocket].into_iter().collect(),
        ping_interval: Duration::from_millis(25000),
        ping_timeout: Duration::from_millis(20000),
        max_payload: 100_000,
    }
}

/// An open packet advertising no upgrade at all.
pub fn open_packet_no_upgrade() -> OpenPacket {
    OpenPacket {
        upgrades: std::iter::empty().collect(),
        ..open_packet()
    }
}

/// Connect a client over polling against the mock, answering the handshake
/// with `open`. Returns the connected client and the scripted server.
pub async fn connect_polling<const N: usize>(
    open: &OpenPacket,
    transports: [TransportType; N],
) -> (Client<MockSvc>, MockServer) {
    let (svc, mut server) = mock();
    let (client, _) = tokio::join!(Client::connect(svc, transports).timeout(), async {
        let call = server.next_http().await;
        assert_eq!(call.method, Method::GET, "handshake must be a GET");
        call.respond_open(open);
    });
    (client.expect("handshake should succeed"), server)
}

/// Connect a client directly over websocket against the mock.
pub async fn connect_ws(open: &OpenPacket) -> (Client<MockSvc>, MockServer, ServerWs) {
    let (svc, mut server) = mock();
    let client = Client::connect(svc, [TransportType::Websocket]).timeout();
    let (client, ws) = tokio::join!(client, async {
        let ws = server.next_ws().await.accept();
        ws.send_packet(Packet::Open(open.clone()));
        ws
    });
    (client.expect("handshake should succeed"), server, ws)
}

/// Split a `\x1e`-separated payload into packets.
pub fn decode_payload(body: &[u8]) -> Vec<Packet> {
    let body = std::str::from_utf8(body).expect("payload should be valid utf8");
    if body.is_empty() {
        return Vec::new();
    }
    body.split(SEP)
        .map(|part| {
            Packet::parse(ProtocolVersion::V4, part.to_owned())
                .expect("payload part should be a valid packet")
        })
        .collect()
}

/// Join packets with the `\x1e` record separator.
pub fn encode_payload(packets: impl IntoIterator<Item = Packet>) -> String {
    packets
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
        .join("\x1e")
}

fn query_param<'a>(uri: &'a Uri, key: &str) -> Option<&'a str> {
    uri.query()?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}
