//! Custom Request Body compat struct implementation to map [`http_body_v1::Body`] to a [`http_body::Body`]
//! Only enabled with the feature flag `hyper-v1`
//!
//! Eavily inspired from : https://github.com/davidpdrsn/tower-hyper-http-body-compat

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use http::HeaderMap;
use pin_project::pin_project;

/// Wraps a body to implement the [`http_body::Body`] on it
#[pin_project]
pub struct IncomingBody<B> {
    #[pin]
    body: B,
    trailers: Option<HeaderMap>,
}
impl<B> IncomingBody<B> {
    pub fn new(body: B) -> IncomingBody<B> {
        IncomingBody {
            body,
            trailers: None,
        }
    }
}

impl<B> http_body::Body for IncomingBody<B>
where
    B: http_body_v1::Body,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match futures::ready!(this.body.poll_frame(cx)) {
            Some(Ok(frame)) => {
                let frame = match frame.into_data() {
                    Ok(data) => return Poll::Ready(Some(Ok(data))),
                    Err(frame) => frame,
                };

                match frame.into_trailers() {
                    Ok(trailers) => {
                        *this.trailers = Some(trailers);
                    }
                    Err(_frame) => {}
                }

                Poll::Ready(None)
            }
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        loop {
            let this = self.as_mut().project();

            if let Some(trailers) = this.trailers.take() {
                break Poll::Ready(Ok(Some(trailers)));
            }

            match futures::ready!(this.body.poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_trailers() {
                    Ok(trailers) => break Poll::Ready(Ok(Some(trailers))),
                    // we might get a trailers frame on next poll
                    // so loop and try again
                    Err(_frame) => {}
                },
                Some(Err(err)) => break Poll::Ready(Err(err)),
                None => break Poll::Ready(Ok(None)),
            }
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        let size_hint = self.body.size_hint();
        let mut out = http_body::SizeHint::new();
        out.set_lower(size_hint.lower());
        if let Some(upper) = size_hint.upper() {
            out.set_upper(upper);
        }
        out
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
}
