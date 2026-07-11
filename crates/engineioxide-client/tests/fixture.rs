use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::service::EngineIoService;
use engineioxide_client::Client;
use engineioxide_client::tungstenite_impl::TokioTungsteniteWebSocket;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::tungstenite::handshake::client::{Request, generate_key};
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_util::io::StreamReader;

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

pub async fn client_ws_connect<H: EngineIoHandler>(
    svc: EngineIoService<H>,
) -> Client<EngineIoService<H>, TokioTungsteniteWebSocket<StreamImpl>> {
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

    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
        StreamImpl::new(tx1, rx),
        Role::Client,
        Default::default(),
    )
    .await;

    tokio::spawn(svc.ws_init(
        StreamImpl::new(tx, rx1),
        engineioxide::ProtocolVersion::V4,
        None,
        parts,
    ));

    Client::connect_ws(ws).await.unwrap()
}
