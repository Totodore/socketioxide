use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::future::BoxFuture;
use futures_util::FutureExt;
use http_body_util::combinators::BoxBody;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::{
    io::{self, ReadBuf},
    sync::mpsc,
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_util::io::StreamReader;

use hyper::service::Service as HyperSvc;

use crate::{flavors::hyper_tungstenite::TokioTungsteniteWS, transport::PollingSvc};

/// Trait alias for [`TestingFlavor`] inner service.
///
/// Typically this wil be satisfied by the engineioxide service.
pub trait EngineSvc:
    PollingSvc<Body: http_body::Body<Data: Send + std::fmt::Debug + 'static>>
    + HyperSvc<
        (StreamImpl, http::Request<()>),
        Response = (),
        Error: std::error::Error + Send + 'static,
        Future: Send,
    > + Send
    + Clone
    + 'static
{
}

impl<Svc> EngineSvc for Svc where
    Svc: PollingSvc<Body: http_body::Body<Data: Send + std::fmt::Debug + 'static>>
        + HyperSvc<
            (StreamImpl, http::Request<()>),
            Response = (),
            Error: std::error::Error + Send + 'static,
            Future: Send,
        > + Send
        + Clone
        + 'static
{
}

#[derive(Debug, Clone)]
pub struct TestingFlavor<Svc> {
    inner: Svc,
}
impl<Svc> TestingFlavor<Svc> {
    pub fn new(inner: Svc) -> Self {
        Self { inner }
    }
}
impl<Svc> From<Svc> for TestingFlavor<Svc> {
    fn from(inner: Svc) -> Self {
        Self { inner }
    }
}

/// HTTP Service implementation
impl<Svc> HyperSvc<http::Request<BoxBody<Bytes, Infallible>>> for TestingFlavor<Svc>
where
    Svc: HyperSvc<http::Request<BoxBody<Bytes, Infallible>>>,
    Svc: Clone,
{
    type Response = Svc::Response;
    type Error = Svc::Error;
    type Future = Svc::Future;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        self.inner.clone().call(req)
    }
}

/// Websocket service implementation
impl<Svc, E, Fut> HyperSvc<http::Request<()>> for TestingFlavor<Svc>
where
    Svc: HyperSvc<(StreamImpl, http::Request<()>), Response = (), Error = E, Future = Fut> + Clone,
    Svc: Clone + Send + 'static,
    E: std::error::Error + Send + 'static,
    Fut: Future<Output = Result<Svc::Response, Svc::Error>> + Send,
{
    type Response = TokioTungsteniteWS<StreamImpl>;
    type Error = tokio_tungstenite::tungstenite::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: http::Request<()>) -> Self::Future {
        let svc = self.inner.clone();
        let (client, server) = duplex_stream();
        tracing::debug!("initializing duplex stream");

        async move {
            svc.call((server, req))
                .await
                .map_err(|e| io::Error::other(e.to_string()))?;

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
