use crate::body::ResponseBody;
use bytes::Bytes;
use futures_core::ready;
use http::header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, UPGRADE};
use http::{HeaderValue, Response, StatusCode};
use http_body::{Body, Full};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

pub(crate) type BoxFuture<B> =
    Pin<Box<dyn Future<Output = Result<Response<ResponseBody<B>>, crate::errors::Error>> + Send>>;

/// Create a response for http request
pub fn http_response<B, D>(
    code: StatusCode,
    data: D,
) -> Result<Response<ResponseBody<B>>, http::Error>
where
    D: Into<Full<Bytes>>,
{
    Response::builder()
        .status(code)
        .body(ResponseBody::custom_response(data.into()))
}

/// Create a response for websocket upgrade
pub fn ws_response<B>(ws_key: &HeaderValue) -> Result<Response<ResponseBody<B>>, http::Error> {
    let derived = derive_accept_key(ws_key.as_bytes());
    let sec = derived.parse::<HeaderValue>().unwrap();
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, HeaderValue::from_static("websocket"))
        .header(CONNECTION, HeaderValue::from_static("Upgrade"))
        .header(SEC_WEBSOCKET_ACCEPT, sec)
        .body(ResponseBody::empty_response())
}

#[pin_project]
pub struct ResponseFuture<F, B> {
    #[pin]
    inner: ResponseFutureInner<F, B>,
}

impl<F, B> ResponseFuture<F, B> {
    pub fn empty_response(code: u16) -> Self {
        Self {
            inner: ResponseFutureInner::EmptyResponse { code },
        }
    }


    pub fn new(future: F) -> Self {
        Self {
            inner: ResponseFutureInner::Future { future },
        }
    }
    pub fn async_response(future: BoxFuture<B>) -> Self {
        Self {
            inner: ResponseFutureInner::AsyncResponse { future },
        }
    }
}
#[pin_project(project = ResFutProj)]
enum ResponseFutureInner<F, B> {
    EmptyResponse {
        code: u16,
    },
    AsyncResponse {
        future: BoxFuture<B>,
    },
    Future {
        #[pin]
        future: F,
    },
}

impl<ResBody, F, E> Future for ResponseFuture<F, ResBody>
where
    ResBody: Body,
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match self.project().inner.project() {
            ResFutProj::Future { future } => ready!(future.poll(cx))?.map(ResponseBody::new),

            ResFutProj::EmptyResponse { code } => Response::builder()
                .status(*code)
                .body(ResponseBody::empty_response())
                .unwrap(),

            ResFutProj::AsyncResponse { future } => ready!(future
                .as_mut()
                .poll(cx)
                .map(|r| r.unwrap_or_else(|e| e.into()))),
        };
        Poll::Ready(Ok(res))
    }
}
