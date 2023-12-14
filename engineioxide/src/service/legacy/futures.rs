use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{ready, Future};
use hyper_legacy::http::Response;
use pin_project::pin_project;

use crate::{body::ResponseBody, errors::Error, service::futures::ResponseFuture};

pub(crate) type BoxFuture<B> =
    Pin<Box<dyn Future<Output = Result<Response<ResponseBody<B>>, Error>> + Send>>;

fn map_res<R>(res: http::Response<R>) -> Response<R> {
    let (mut parts, body) = res.into_parts();
    let mut res = Response::builder()
        .status(parts.status.as_u16())
        .version(match parts.version {
            http::Version::HTTP_09 => hyper_legacy::Version::HTTP_09,
            http::Version::HTTP_10 => hyper_legacy::Version::HTTP_10,
            http::Version::HTTP_11 => hyper_legacy::Version::HTTP_11,
            http::Version::HTTP_2 => hyper_legacy::Version::HTTP_2,
            http::Version::HTTP_3 => hyper_legacy::Version::HTTP_3,
        })
        .body(body)
        .unwrap();

    for (k, v) in parts.headers.iter() {
        let v = hyper_legacy::header::HeaderValue::from_bytes(v.as_bytes()).unwrap();
        res.headers_mut().insert(k.as_str(), v);
    }
    res
}

#[pin_project]
pub struct LegacyResponseFuture<F, B>(#[pin] ResponseFuture<F, B>);

impl<F, B> LegacyResponseFuture<F, B> {
    pub fn new(res: ResponseFuture<F, B>) -> Self {
        LegacyResponseFuture(res)
    }
}

impl<F, B, E> Future for LegacyResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<ResponseBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.project().0.poll(cx))?;
        Poll::Ready(Ok(map_res(res)))
    }
}
