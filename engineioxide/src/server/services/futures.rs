use crate::errors::Error;
use crate::server::body::response::ResponseBody;
use futures::ready;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) type BoxFuture<B> =
    Pin<Box<dyn Future<Output = Result<Response<ResponseBody<B>>, Error>> + Send>>;

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
    pub fn ready(res: Result<Response<ResponseBody<B>>, Error>) -> Self {
        Self {
            inner: ResponseFutureInner::ReadyResponse { res: Some(res) },
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
    ReadyResponse {
        res: Option<Result<Response<ResponseBody<B>>, Error>>,
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
            ResFutProj::ReadyResponse { res } => res.take().unwrap().unwrap_or_else(|e| e.into()),
        };
        Poll::Ready(Ok(res))
    }
}
