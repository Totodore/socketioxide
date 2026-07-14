use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use engineioxide::{ProtocolVersion, handler::EngineIoHandler, service::EngineIoService};
use engineioxide_core::Sid;
use futures_core::future::BoxFuture;
use futures_util::FutureExt;
use http::Response;
use http_body_util::combinators::BoxBody;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{
    io::{self, ReadBuf},
    sync::mpsc,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::tungstenite::{self, protocol::Role};
use tokio_util::io::StreamReader;

use crate::flavors::hyper_tungstenite::TokioTungsteniteWS;

/// Create a stub TCP Stream implemented with two unbounded channels
fn duplex_stream() -> (StreamImpl, StreamImpl) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (tx1, rx1) = mpsc::unbounded_channel();
    (StreamImpl::new(tx, rx1), StreamImpl::new(tx1, rx))
}

pin_project_lite::pin_project! {
    /// Half of a full-duplex bytes stream used to fake
    /// a TCP stream to use [`tokio_tungstenite`] in local.
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

#[derive(Debug)]
pub struct EngineIoTestSvc<H: EngineIoHandler> {
    inner: EngineIoService<H>,
}
impl<H: EngineIoHandler> Clone for EngineIoTestSvc<H> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<H: EngineIoHandler> From<EngineIoService<H>> for EngineIoTestSvc<H> {
    fn from(inner: EngineIoService<H>) -> Self {
        Self { inner }
    }
}

/// HTTP Service implementation
impl<H: EngineIoHandler> hyper::service::Service<http::Request<BoxBody<Bytes, Infallible>>>
    for EngineIoTestSvc<H>
{
    type Response = Response<BoxBody<Bytes, Infallible>>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        let svc = self.inner.clone();
        async move { svc.call(req).await.map(|r| r.map(BoxBody::new)) }.boxed()
    }
}

/// Websocket service implementation
impl<H: EngineIoHandler> hyper::service::Service<http::Request<()>> for EngineIoTestSvc<H> {
    type Response = TokioTungsteniteWS<StreamImpl>;
    type Error = tungstenite::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: http::Request<()>) -> Self::Future {
        let (parts, _) = req.into_parts();
        let svc = self.inner.clone();
        let (client, server) = duplex_stream();
        tracing::debug!("initializing duplex stream");
        let query = parts.uri.query().unwrap();

        let sid: Option<Sid> = query
            .split('&')
            .find(|s| s.starts_with("sid="))
            .and_then(|s| s.split('=').nth(1).map(|s1| s1.parse().ok()))
            .flatten();

        async move {
            tokio::spawn(svc.ws_init(server, ProtocolVersion::V4, sid, parts));

            tracing::debug!("server connected, wiring websocket client");

            let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
                client,
                Role::Client,
                Default::default(),
            )
            .await;

            tracing::debug!("stub ws client wired up");

            Ok(TokioTungsteniteWS::from(ws))
        }
        .boxed()
    }
}
