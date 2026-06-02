//! End-to-end memory-leak regression test for the abandoned websocket-upgrade scenario.
//!
//! Reproduces the production leak from socketioxide task #214 over **real TCP connections**
//! against a **real hyper server**:
//!
//! 1. A client opens an engine.io HTTP long-polling session (handshake).
//! 2. It then starts a websocket upgrade for that session but never completes the probe
//!    handshake (`2probe` / `5`), while keeping the TCP connection open — mimicking a reverse
//!    proxy that forwards the upgrade as a pooled request and never relays the `101`, after
//!    which the browser falls back to polling.
//!
//! While a session is upgrading its heartbeat is paused, so before the fix the per-session state
//! (engine `Socket`, both mpsc channels, the heartbeat task and the hyper connection buffer) was
//! retained forever and the server RSS grew without bound under connect/abandon churn.
//!
//! This test creates many such abandoned-upgrade sessions, keeps every client connection open,
//! and asserts that every session is nonetheless reclaimed once the upgrade timeout elapses.
//! If the upgrade-timeout fix is removed this test fails (the sessions never disconnect).

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::WebSocketStream;

/// Tracks the number of live engine.io sessions on the server.
#[derive(Debug, Default)]
struct CountingHandler {
    /// Sessions currently held by the engine (incremented on connect, decremented on disconnect).
    live: AtomicUsize,
    /// Total sessions ever created.
    connects: AtomicUsize,
}

impl EngineIoHandler for CountingHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, _socket: Arc<Socket<()>>) {
        self.live.fetch_add(1, Ordering::SeqCst);
        self.connects.fetch_add(1, Ordering::SeqCst);
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, _reason: DisconnectReason) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
    fn on_message(self: &Arc<Self>, _msg: Str, _socket: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

/// Spawn a real engine.io server on an ephemeral port and return its address.
async fn spawn_server(handler: Arc<CountingHandler>, config: EngineIoConfig) -> SocketAddr {
    let svc = EngineIoService::with_config(handler, config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };
            let io = TokioIo::new(stream);
            let svc = svc.clone();
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(io, svc)
                    .with_upgrades()
                    .await;
            });
        }
    });

    addr
}

/// Perform a polling handshake over a throwaway connection and return the new session id.
async fn handshake(addr: SocketAddr) -> Option<String> {
    let mut stream = TcpStream::connect(addr).await.ok()?;
    let req = "GET /engine.io/?EIO=4&transport=polling HTTP/1.1\r\n\
               Host: localhost\r\n\
               Connection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).await.ok()?;

    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf))
        .await
        .ok()?
        .ok()?;

    let body = String::from_utf8_lossy(&buf);
    let start = body.find("\"sid\":\"")? + "\"sid\":\"".len();
    let end = body[start..].find('"')? + start;
    Some(body[start..end].to_string())
}

/// Open a websocket upgrade for an existing session and **never** complete the probe handshake.
/// The returned stream is held by the caller so the underlying TCP connection stays open.
async fn stall_upgrade(addr: SocketAddr, sid: &str) -> Option<WebSocketStream<TcpStream>> {
    let tcp = TcpStream::connect(addr).await.ok()?;
    let url = format!("ws://{addr}/engine.io/?EIO=4&transport=websocket&sid={sid}");
    let (ws, _resp) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::client_async(url.as_str(), tcp),
    )
    .await
    .ok()?
    .ok()?;
    Some(ws)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abandoned_ws_upgrades_do_not_leak_sessions() {
    const N: usize = 100;

    let handler = Arc::new(CountingHandler::default());
    // Long heartbeat so it cannot reclaim sessions on its own (isolating the upgrade-timeout path),
    // and a short upgrade timeout so abandoned upgrades are reclaimed quickly.
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_secs(60))
        .ping_timeout(Duration::from_secs(60))
        .upgrade_timeout(Duration::from_secs(1))
        .build();
    let addr = spawn_server(handler.clone(), config).await;

    // Concurrently create N sessions, each stuck mid-upgrade, keeping every client connection open.
    let mut tasks = Vec::with_capacity(N);
    for _ in 0..N {
        tasks.push(tokio::spawn(async move {
            let sid = handshake(addr).await?;
            stall_upgrade(addr, &sid).await
        }));
    }
    let mut held = Vec::with_capacity(N);
    for task in tasks {
        if let Ok(Some(ws)) = task.await {
            held.push(ws);
        }
    }

    let created = held.len();
    let connects = handler.connects.load(Ordering::SeqCst);
    let peak_live = handler.live.load(Ordering::SeqCst);
    assert_eq!(
        created, N,
        "only {created}/{N} abandoned-upgrade sessions were created"
    );
    assert_eq!(connects, N, "server saw {connects} connects, expected {N}");
    assert!(
        peak_live > 0,
        "expected live sessions while upgrades are stuck, got {peak_live}"
    );

    // Every client connection is still open and none has sent the upgrade probe. Without an upgrade
    // timeout the sessions would stay live forever (the leak). Wait past the timeout and confirm
    // they are all reclaimed.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while handler.live.load(Ordering::SeqCst) > 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let live = handler.live.load(Ordering::SeqCst);
    assert_eq!(
        live, 0,
        "leaked {live}/{N} abandoned-upgrade sessions (peak live {peak_live}) — \
         sessions were not reclaimed after the upgrade timeout"
    );

    // Keep the client connections alive until the very end so the server truly had to reclaim the
    // sessions on its own rather than because the client closed the socket.
    drop(held);
}
