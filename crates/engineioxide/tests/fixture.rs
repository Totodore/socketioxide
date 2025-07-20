use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use engineioxide::{
    ProtocolVersion, config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService,
    socket::Sid,
};
use http::Request;
use http_body_util::{BodyExt, Either, Empty, Full};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::mpsc,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{handshake::client::generate_key, protocol::Role},
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
pub fn send_req<H: EngineIoHandler>(
    svc: &mut EngineIoService<H>,
    params: String,
    method: http::Method,
    body: Option<String>,
) -> impl Future<Output = String> + 'static {
    let body = match body {
        Some(b) => Either::Left(Full::new(VecDeque::from(b.into_bytes()))),
        None => Either::Right(Empty::<VecDeque<u8>>::new()),
    };

    let req = Request::builder()
        .method(method)
        .uri(format!("http://127.0.0.1/engine.io/?EIO=4&{params}"))
        .body(body)
        .unwrap();
    let res = svc.call(req);
    async move {
        let body = res
            .await
            .unwrap()
            .body_mut()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        String::from_utf8(body.to_vec())
            .unwrap()
            .chars()
            .skip(1)
            .collect()
    }
}

pub async fn create_polling_connection<H: EngineIoHandler>(svc: &mut EngineIoService<H>) -> String {
    let body = send_req(
        svc,
        "transport=polling".to_string(),
        http::Method::GET,
        None,
    )
    .await;
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();
    open_packet.sid
}
pub async fn create_ws_connection<H: EngineIoHandler>(
    svc: &mut EngineIoService<H>,
) -> WebSocketStream<StreamImpl> {
    new_ws_mock_conn(svc, ProtocolVersion::V4, None).await
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
async fn new_ws_mock_conn<H: EngineIoHandler>(
    svc: &mut EngineIoService<H>,
    protocol: ProtocolVersion,
    sid: Option<Sid>,
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
        .uri("ws://127.0.0.1/engine.io/?EIO=4&transport=websocket")
        .body(http_body_util::Empty::<Bytes>::new())
        .unwrap()
        .into_parts()
        .0;
    tokio::spawn(svc.ws_init(StreamImpl::new(tx, rx1), protocol, sid, parts));

    tokio_tungstenite::WebSocketStream::from_raw_socket(
        StreamImpl::new(tx1, rx),
        Role::Client,
        Default::default(),
    )
    .await
}

pub async fn create_server<H: EngineIoHandler>(handler: H) -> EngineIoService<H> {
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();

    EngineIoService::with_config(Arc::new(handler), config)
}
