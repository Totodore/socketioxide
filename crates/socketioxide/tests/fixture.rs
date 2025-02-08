#![allow(dead_code)]

use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use engineioxide::service::NotFoundService;
use futures_util::SinkExt;
use http::Request;
use http_body_util::{BodyExt, Either, Empty, Full};

use serde::{Deserialize, Serialize};
use socketioxide::{service::SocketIoService, ProtocolVersion, SocketIo};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::mpsc,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::{
    tungstenite::{handshake::client::generate_key, protocol::Role, Message},
    WebSocketStream,
};
use tokio_util::io::StreamReader;
use tower_service::Service;

/// An OpenPacket is used to initiate a connection
#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
struct OpenPacket {
    sid: String,
    upgrades: Vec<String>,
    ping_interval: u64,
    ping_timeout: u64,
    max_payload: u64,
}

/// Params should be in the form of `key1=value1&key2=value2`
pub async fn send_req(
    svc: &SocketIoService<NotFoundService>,
    params: String,
    method: http::Method,
    body: Option<String>,
) -> String {
    let body = match body {
        Some(b) => Either::Left(Full::new(VecDeque::from(b.into_bytes()))),
        None => Either::Right(Empty::<VecDeque<u8>>::new()),
    };
    let req = Request::builder()
        .method(method)
        .uri(format!("http://127.0.0.1/socket.io/?EIO=4&{}", params))
        .body(body)
        .unwrap();
    let mut res = svc.clone().call(req).await.unwrap();
    let is_json = res
        .headers()
        .get("Content-Type")
        .map(|v| v == "application/json")
        .unwrap_or_default();
    let body = res.body_mut().collect().await.unwrap().to_bytes();
    if is_json {
        String::from_utf8(body.to_vec()).unwrap()
    } else {
        String::from_utf8(body.to_vec())
            .unwrap()
            .chars()
            .skip(1)
            .collect()
    }
}

pub async fn create_polling_connection(svc: &SocketIoService<NotFoundService>) -> String {
    let body = send_req(
        svc,
        "transport=polling".to_string(),
        http::Method::GET,
        None,
    )
    .await;
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();

    send_req(
        svc,
        format!("transport=polling&sid={}", open_packet.sid),
        http::Method::POST,
        Some("40{}".to_string()),
    )
    .await;

    // wait for the server to process messages and call handlers
    tokio::time::sleep(Duration::from_millis(10)).await;

    open_packet.sid
}

pin_project_lite::pin_project! {
    pub struct StreamImpl {
        tx: mpsc::UnboundedSender<Result<Bytes, io::Error>>,
        #[pin]
        rx: StreamReader<UnboundedReceiverStream<Result<Bytes, io::Error>>, Bytes>,
    }
}
impl StreamImpl {
    pub fn new(
        tx: mpsc::UnboundedSender<Result<Bytes, io::Error>>,
        rx: mpsc::UnboundedReceiver<Result<Bytes, io::Error>>,
    ) -> Self {
        Self {
            tx,
            rx: StreamReader::new(UnboundedReceiverStream::new(rx)),
        }
    }
}

impl AsyncRead for StreamImpl {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().rx.poll_read(cx, buf)
    }
}
impl AsyncWrite for StreamImpl {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let len = buf.len();
        self.project()
            .tx
            .send(Ok(Bytes::copy_from_slice(buf)))
            .unwrap();
        Poll::Ready(Ok(len))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub async fn create_ws_connection(
    svc: &SocketIoService<NotFoundService>,
) -> WebSocketStream<StreamImpl> {
    let (tx, rx) = mpsc::unbounded_channel();
    let (tx1, rx1) = mpsc::unbounded_channel();

    let parts = Request::builder()
        .method("GET")
        .header("Host", "127.0.0.1")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .uri("ws://127.0.0.1/socket.io/?EIO=4&transport=websocket")
        .body(http_body_util::Empty::<Bytes>::new())
        .unwrap()
        .into_parts()
        .0;
    tokio::spawn(svc.ws_init(StreamImpl::new(tx, rx1), ProtocolVersion::V5, None, parts));

    let mut ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
        StreamImpl::new(tx1, rx),
        Role::Client,
        Default::default(),
    )
    .await;
    ws.send(Message::text("40{}")).await.unwrap();
    // wait for the server to process messages and call handlers
    tokio::time::sleep(Duration::from_millis(10)).await;
    ws
}

pub async fn create_server() -> (SocketIoService<NotFoundService>, SocketIo) {
    SocketIo::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_buffer_size(100000)
        .build_svc()
}
