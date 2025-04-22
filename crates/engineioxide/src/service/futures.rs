use crate::body::ResponseBody;
use crate::errors::Error;
use futures_core::ready;
use http::Response;
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) type BoxFuture<B> =
    Pin<Box<dyn Future<Output = Result<Response<ResponseBody<B>>, Error>> + Send>>;

pin_project! {
    #[project = ResFutProj]
    pub enum ResponseFuture<F, B> {
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
}

impl<F, B> ResponseFuture<F, B> {
    pub fn empty_response(code: u16) -> Self {
        ResponseFuture::EmptyResponse { code }
    }
    pub fn ready(res: Result<Response<ResponseBody<B>>, Error>) -> Self {
        ResponseFuture::ReadyResponse { res: Some(res) }
    }
    pub fn new(future: F) -> Self {
        ResponseFuture::Future { future }
    }
    pub fn async_response(future: BoxFuture<B>) -> Self {
        ResponseFuture::AsyncResponse { future }
    }
}

impl<ResBody, F, E> Future for ResponseFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match self.project() {
            ResFutProj::Future { future } => ready!(future.poll(cx))?.map(ResponseBody::new),

            ResFutProj::EmptyResponse { code } => Response::builder()
                .status(*code)
                .body(ResponseBody::empty_response())
                .unwrap(),
            ResFutProj::AsyncResponse { future } => ready!(
                future
                    .as_mut()
                    .poll(cx)
                    .map(|r| r.unwrap_or_else(|e| e.into()))
            ),
            ResFutProj::ReadyResponse { res } => res.take().unwrap().unwrap_or_else(|e| e.into()),
        };
        Poll::Ready(Ok(res))
    }
}
