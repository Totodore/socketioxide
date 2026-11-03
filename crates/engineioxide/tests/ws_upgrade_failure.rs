//! A failed websocket upgrade must only close the websocket transport:
//! the polling transport of the session is still valid and must keep working.
//! A real http server is required to test the whole upgrade path.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, protocol::CloseFrame},
};

mod fixture;
use fixture::{create_polling_connection, send_req};

const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
struct MyHandler {
    socket_tx: mpsc::UnboundedSender<Arc<Socket<()>>>,
    disconnect_tx: mpsc::UnboundedSender<DisconnectReason>,
}

impl EngineIoHandler for MyHandler {
    type Data = ();

    fn on_connect(self: Arc<Self>, socket: Arc<Socket<()>>) {
        self.socket_tx.send(socket).unwrap();
    }
    fn on_disconnect(&self, _socket: Arc<Socket<()>>, reason: DisconnectReason) {
        self.disconnect_tx.send(reason).unwrap();
    }
    fn on_message(self: &Arc<Self>, _msg: Str, _socket: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _data: Bytes, _socket: Arc<Socket<()>>) {}
}

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct Server {
    svc: EngineIoService<MyHandler>,
    addr: std::net::SocketAddr,
    sid: String,
    socket: Arc<Socket<()>>,
    disconnect_rx: mpsc::UnboundedReceiver<DisconnectReason>,
}

impl Server {
    /// Start a http server with a polling session.
    async fn start() -> Self {
        let (socket_tx, mut socket_rx) = mpsc::unbounded_channel();
        let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel();
        let config = EngineIoConfig::builder()
            .ping_interval(Duration::from_secs(60))
            .ping_timeout(Duration::from_secs(60))
            .upgrade_timeout(Duration::from_secs(30))
            .build();
        let handler = Arc::new(MyHandler {
            socket_tx,
            disconnect_tx,
        });
        let mut svc = EngineIoService::with_config(handler, config);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let http_svc = svc.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let conn = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), http_svc.clone())
                    .with_upgrades();
                tokio::spawn(conn);
            }
        });

        let sid = create_polling_connection(&mut svc).await;
        let socket = socket_rx.recv().await.unwrap();
        Self {
            svc,
            addr,
            sid,
            socket,
            disconnect_rx,
        }
    }

    /// Start a websocket upgrade of the session.
    async fn upgrade(&self) -> Ws {
        let url = format!(
            "ws://{}/engine.io/?EIO=4&transport=websocket&sid={}",
            self.addr, self.sid
        );
        let (ws, _) = connect_async(url).await.unwrap();
        // Give the server the time to process the 101 and to start its handshake.
        tokio::time::sleep(Duration::from_millis(20)).await;
        ws
    }

    async fn poll(&mut self) -> String {
        let params = format!("transport=polling&sid={}", self.sid);
        let res = send_req(&mut self.svc, params, http::Method::GET, None);
        tokio::time::timeout(TIMEOUT, res)
            .await
            .expect("the polling request should return")
    }

    /// Assert that the session and its polling transport are still working
    async fn assert_polling_works(&mut self) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            self.disconnect_rx.try_recv().is_err(),
            "the session should not be closed"
        );

        // The first response may be the NOOP packet sent at the start of the upgrade.
        // While the session is upgrading, a new one is returned by every request.
        self.socket.emit(Str::from("hello")).unwrap();
        let mut data = self.poll().await;
        if data.is_empty() {
            data = self.poll().await;
        }
        assert!(data.ends_with("hello"), "{data:?}");
    }
}

/// `send_req` removes the first byte of the response.
fn is_noop(data: &str) -> bool {
    data.is_empty()
}

async fn wait_closed(ws: &mut Ws) {
    tokio::time::timeout(TIMEOUT, async {
        while let Some(Ok(_)) = ws.next().await {}
    })
    .await
    .expect("the websocket should be closed");
}

#[tokio::test]
async fn client_closing_during_the_probe_keeps_the_session() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.close(None::<CloseFrame>).await.unwrap();
    wait_closed(&mut ws).await;

    server.assert_polling_works().await;
}

#[tokio::test]
async fn bad_probe_packet_keeps_the_session() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.send(Message::text("4not-a-probe")).await.unwrap();
    wait_closed(&mut ws).await;

    server.assert_polling_works().await;
}

#[tokio::test]
async fn invalid_probe_packet_keeps_the_session() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.send(Message::text("z")).await.unwrap();
    wait_closed(&mut ws).await;

    server.assert_polling_works().await;
}

#[tokio::test]
async fn binary_probe_keeps_the_session() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.send(Message::binary(Bytes::from_static(b"2probe")))
        .await
        .unwrap();
    wait_closed(&mut ws).await;

    server.assert_polling_works().await;
}

#[tokio::test]
async fn failure_after_the_ping_probe_keeps_the_session() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.send(Message::text("2probe")).await.unwrap();
    let pong = tokio::time::timeout(TIMEOUT, ws.next()).await.unwrap();
    assert_eq!(pong.unwrap().unwrap(), Message::text("3probe"));

    // Should be an upgrade packet
    ws.send(Message::text("4message")).await.unwrap();
    wait_closed(&mut ws).await;

    server.assert_polling_works().await;
}

#[tokio::test]
async fn polling_is_not_answered_with_noop_after_a_failed_upgrade() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.close(None::<CloseFrame>).await.unwrap();
    wait_closed(&mut ws).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The NOOP packet of the failed upgrade, then a regular long polling request.
    assert!(is_noop(&server.poll().await));
    let params = format!("transport=polling&sid={}", server.sid);
    let pending = send_req(&mut server.svc, params, http::Method::GET, None);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), pending)
            .await
            .is_err(),
        "the request should wait for a packet"
    );
}

#[tokio::test]
async fn upgrade_can_be_retried_after_a_failure() {
    let mut server = Server::start().await;
    let mut ws = server.upgrade().await;
    ws.send(Message::text("z")).await.unwrap();
    wait_closed(&mut ws).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Consume the NOOP packet of the failed upgrade to release the socket.
    assert!(is_noop(&server.poll().await));

    let mut ws = server.upgrade().await;
    ws.send(Message::text("2probe")).await.unwrap();
    let pong = tokio::time::timeout(TIMEOUT, ws.next()).await.unwrap();
    assert_eq!(pong.unwrap().unwrap(), Message::text("3probe"));
    ws.send(Message::text("5")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    server.socket.emit(Str::from("hello")).unwrap();
    let msg = tokio::time::timeout(TIMEOUT, ws.next()).await.unwrap();
    let mut msg = msg.unwrap().unwrap();
    // The NOOP packet of the second upgrade is not sent on the websocket.
    if msg == Message::text("6") {
        msg = tokio::time::timeout(TIMEOUT, ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    assert_eq!(msg, Message::text("4hello"));
}

/// A second upgrade request must not disturb the upgrade in progress.
#[tokio::test]
async fn concurrent_upgrade_does_not_disturb_the_first_one() {
    let mut server = Server::start().await;
    let mut first = server.upgrade().await;
    let mut second = server.upgrade().await;

    wait_closed(&mut second).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(server.disconnect_rx.try_recv().is_err());

    first.send(Message::text("2probe")).await.unwrap();
    let pong = tokio::time::timeout(TIMEOUT, first.next()).await.unwrap();
    assert_eq!(pong.unwrap().unwrap(), Message::text("3probe"));
    first.send(Message::text("5")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    server.socket.emit(Str::from("hello")).unwrap();
    let msg = tokio::time::timeout(TIMEOUT, first.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(msg, Message::text("4hello"));
}
