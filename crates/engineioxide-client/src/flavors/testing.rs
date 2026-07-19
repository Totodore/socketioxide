use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::{future::BoxFuture, ready};
use futures_util::FutureExt;
use http_body_util::combinators::BoxBody;
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;
use tokio::io;
use tokio_tungstenite::tungstenite::protocol::Role;

use crate::{flavors::hyper_tungstenite::TokioTungsteniteWS, transport::PollingSvc};

/// Trait alias for [`TestingFlavor`] inner service.
///
/// Typically this wil be satisfied by the engineioxide service.
pub trait EngineSvc:
    PollingSvc<Body: http_body::Body<Data: Send + std::fmt::Debug + 'static>>
    + HyperSvc<
        (DuplexStream, http::Request<()>),
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
            (DuplexStream, http::Request<()>),
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
    Svc:
        HyperSvc<(DuplexStream, http::Request<()>), Response = (), Error = E, Future = Fut> + Clone,
    Svc: Clone + Send + 'static,
    E: std::error::Error + Send + 'static,
    Fut: Future<Output = Result<Svc::Response, Svc::Error>> + Send,
{
    type Response = TokioTungsteniteWS<DuplexStream>;
    type Error = tokio_tungstenite::tungstenite::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, req: http::Request<()>) -> Self::Future {
        let svc = self.inner.clone();
        let (client, server) = DuplexStream::new();
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
pin_project! {
    pub struct DuplexStream {
        #[pin]
        inner: io::DuplexStream,
    }
}
impl DuplexStream {
    fn new() -> (DuplexStream, DuplexStream) {
        let (st1, st2) = io::duplex(usize::MAX);
        let st1 = DuplexStream { inner: st1 };
        let st2 = DuplexStream { inner: st2 };
        (st1, st2)
    }
}

impl io::AsyncRead for DuplexStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl io::AsyncWrite for DuplexStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let len = buf.len();

        // Drop the error to match a real TCP socket which won't
        // immediately error on close.
        let _ = ready!(self.project().inner.poll_write(cx, buf));
        Poll::Ready(Ok(len))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}
