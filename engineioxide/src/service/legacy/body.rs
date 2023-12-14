use std::{
    pin::Pin,
    task::{Context, Poll},
};

use http_body_legacy::Body;
use pin_project::pin_project;

#[pin_project]
pub struct IncomingBody<B>(#[pin] B);
impl<B> IncomingBody<B> {
    pub fn new(body: B) -> Self {
        IncomingBody(body)
    }
}
impl<B> http_body::Body for IncomingBody<B>
where
    B: Body,
{
    type Data = B::Data;

    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.as_mut().project().0.poll_data(cx) {
            Poll::Ready(Some(Ok(buf))) => Poll::Ready(Some(Ok(http_body::Frame::data(buf)))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        let size_hint = self.0.size_hint();
        let mut out = http_body::SizeHint::new();
        out.set_lower(size_hint.lower());
        if let Some(upper) = size_hint.upper() {
            out.set_upper(upper);
        }
        out
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}
