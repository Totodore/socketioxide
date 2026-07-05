//! Tests ensuring that a packet peeked but rejected because it would exceed the
//! configured `max_payload` is not lost: it must be sent on the next polling
//! request, in order, rather than dropped together with the encoder's peek
//! buffer.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use tokio::sync::mpsc;

mod fixture;

use fixture::{create_polling_connection, send_req_raw};

/// A handler that forwards every connected socket to the test so it can emit on it directly.
#[derive(Debug, Clone)]
struct SocketHandler {
    socket_tx: mpsc::Sender<Arc<Socket<()>>>,
}

impl EngineIoHandler for SocketHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        self.socket_tx.try_send(socket).unwrap();
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, _reason: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, _msg: Str, _socket: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

/// Build a service with a small `max_payload` (so payloads are truncated after a
/// single packet) and heartbeat timings large enough not to interfere.
async fn create_small_payload_server(
    max_payload: u64,
) -> (
    EngineIoService<SocketHandler>,
    mpsc::Receiver<Arc<Socket<()>>>,
) {
    let (socket_tx, socket_rx) = mpsc::channel(1);
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .max_payload(max_payload)
        .build();
    let svc = EngineIoService::with_config(Arc::new(SocketHandler { socket_tx }), config);
    (svc, socket_rx)
}

/// Drain the ping packet that the heartbeat job buffers right after connecting,
/// so subsequent polls only observe the packets emitted by the test.
async fn drain_initial_ping<H: EngineIoHandler>(svc: &mut EngineIoService<H>, sid: &str) {
    let ping = poll(svc, sid).await;
    assert_eq!(ping, "2", "expected the initial heartbeat ping");
}

/// Perform a polling request and return the raw payload, failing fast instead of
/// hanging if the payload is unexpectedly empty (e.g. a peeked packet was lost).
async fn poll<H: EngineIoHandler>(svc: &mut EngineIoService<H>, sid: &str) -> String {
    let fut = send_req_raw(
        svc,
        format!("transport=polling&sid={sid}"),
        http::Method::GET,
        None,
    );
    tokio::time::timeout(Duration::from_secs(1), fut)
        .await
        .expect("polling request timed out (a peeked packet was likely lost)")
}

/// A packet rejected for exceeding `max_payload` must be delivered by the next poll.
#[tokio::test]
async fn polling_peeked_packet_sent_on_next_poll() {
    // "4hello" is 6 bytes; two of them (+ separator) exceed the limit, so the
    // first poll emits one packet and peeks-then-rejects the second.
    let (mut svc, mut socket_rx) = create_small_payload_server(10).await;
    let sid = create_polling_connection(&mut svc).await;
    let socket = socket_rx.recv().await.unwrap();

    drain_initial_ping(&mut svc, &sid).await;

    socket.emit("hello").unwrap();
    socket.emit("world").unwrap();

    // First poll: only the first packet fits, the second is peeked and rejected.
    assert_eq!(poll(&mut svc, &sid).await, "4hello");
    // Second poll: the rejected packet must be delivered here, not lost.
    assert_eq!(poll(&mut svc, &sid).await, "4world");
}

/// Several packets rejected across consecutive polls must keep their order.
#[tokio::test]
async fn polling_peeked_packets_preserve_order() {
    let (mut svc, mut socket_rx) = create_small_payload_server(10).await;
    let sid = create_polling_connection(&mut svc).await;
    let socket = socket_rx.recv().await.unwrap();

    drain_initial_ping(&mut svc, &sid).await;

    socket.emit("hello").unwrap();
    socket.emit("world").unwrap();
    socket.emit("there").unwrap();

    // Each poll drains exactly one packet, peeking and rejecting the next one.
    assert_eq!(poll(&mut svc, &sid).await, "4hello");
    assert_eq!(poll(&mut svc, &sid).await, "4world");
    assert_eq!(poll(&mut svc, &sid).await, "4there");
}
