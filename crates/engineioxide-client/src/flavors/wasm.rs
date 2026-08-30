use std::{
    convert::Infallible,
    future::Ready,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use futures_channel::{mpsc, oneshot};
use futures_core::Stream;
use futures_util::{FutureExt, Sink};
use http_body::Frame;
use http_body_util::{BodyExt, combinators::BoxBody};
use pin_project_lite::pin_project;
use web_sys::{
    Headers, ReadableStream, ReadableStreamDefaultReader, RequestInit, Response as FetchResponse,
    WebSocket,
    js_sys::{self, Reflect, Uint8Array, futures::JsFuture},
    wasm_bindgen::{JsCast, JsValue},
};

use crate::transport::ws::WsMessage;

#[derive(Debug, Clone, Default)]
pub struct WasmFlavor;

/// HTTP Service implementation
impl hyper::service::Service<http::Request<BoxBody<Bytes, Infallible>>> for WasmFlavor {
    type Response = http::Response<WasmResBody>;
    type Error = JsValue;
    type Future = WasmResponseFuture;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let uri = parts.uri.to_string();
        let opts = RequestInit::new();
        opts.set_method(parts.method.as_str());
        opts.set_headers(&header_map_to_jsvalue(&parts.headers));

        //TODO: proper readable stream implementation
        //TODO: machinery for box body collection here
        let mut v = body
            .collect()
            .now_or_never()
            .unwrap()
            .unwrap()
            .to_bytes()
            .to_vec();
        opts.set_body_opt_u8_slice(Some(&mut v));

        let inner = web_sys::window()
            .unwrap()
            .fetch_with_str_and_init(&uri, &opts)
            .into_future();

        WasmResponseFuture { inner }
    }
}

fn header_map_to_jsvalue(map: &http::HeaderMap) -> JsValue {
    let headers = Headers::new().unwrap();
    // ignore non-str headers
    for (k, v) in map.iter().filter_map(|(k, v)| Some((k, v.to_str().ok()?))) {
        headers.set(k.as_str(), v).unwrap();
    }

    headers.into()
}
pin_project! {
    pub struct WasmResponseFuture {
        #[pin]
        inner: JsFuture<JsValue>,
    }
}

impl Future for WasmResponseFuture {
    type Output = Result<http::Response<WasmResBody>, JsValue>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res: FetchResponse = ready!(self.project().inner.poll(cx))?.unchecked_into();
        let body = res.body().unwrap();

        let mut response = http::response::Builder::new().status(res.status());

        for entry in res.headers().entries() {
            let pair: js_sys::Array = entry?.unchecked_into();
            let key = pair.get(0).as_string().unwrap();
            let value = pair.get(1).as_string().unwrap();
            response = response.header(key, value);
        }

        Poll::Ready(Ok(response.body(WasmResBody::new(body)).unwrap()))
    }
}

pin_project! {
    pub struct WasmResBody {
        body: ReadableStream,
        reader: ReadableStreamDefaultReader,
        #[pin]
        next: JsFuture,
    }

    impl PinnedDrop for WasmResBody {
        fn drop(this: Pin<&mut Self>) {
            this.project().reader.release_lock();
        }
    }
}

impl WasmResBody {
    fn new(body: ReadableStream) -> Self {
        let reader: ReadableStreamDefaultReader = body.get_reader().unchecked_into();
        let next = reader.read().into_future();
        Self { body, reader, next }
    }
}

impl http_body::Body for WasmResBody {
    type Data = Bytes;
    type Error = JsValue;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();
        let res = ready!(this.next.as_mut().poll(cx))?;

        if Reflect::get(&res, &JsValue::from_str("done"))
            .unwrap()
            .is_falsy()
        {
            return Poll::Ready(None);
        }

        this.next.set(this.reader.read().into_future());
        cx.waker().wake_by_ref();

        let frame = Reflect::get(&res, &JsValue::from_str("value"))
            .unwrap()
            .unchecked_into::<Uint8Array>()
            .to_vec();

        Poll::Ready(Some(Ok(Frame::data(Bytes::from(frame)))))
    }
}

/// WS Service Implementation
impl hyper::service::Service<http::Request<()>> for WasmFlavor {
    type Response = WasmWebsocket;
    type Error = JsValue;
    type Future = ConnectWebSocketFut;

    fn call(&self, req: http::Request<()>) -> Self::Future {
        let uri = req.uri().to_string();
        std::future::ready(WebSocket::new(&uri).map(WasmWebsocket::new))
    }
}

pin_project! {
    pub struct ConnectWebSocketFut {
        ws: Option<WasmWebsocket>
    }
}

impl ConnectWebSocketFut {
    fn new(ws: WebSocket) -> Self {
        ws.set_on
    }
}

impl Future for ConnectWebSocketFut {
    type Output = Result<WasmWebsocket, JsValue>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().project().connect_recv.poll(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(self.ws.take().unwrap())),
            Poll::Ready(Err(_cancelled)) => todo!("err"),
            Poll::Pending => match self.project().error_recv.poll(cx) {
                Poll::Ready(Ok(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Err(_cancelled)) => todo!("err"),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

pin_project! {
    pub struct WasmWebsocket {
        #[pin]
        inner: web_sys::WebSocket,

        #[pin]
        connect_recv: oneshot::Receiver<()>,
        #[pin]
        close_recv: oneshot::Receiver<()>,
        #[pin]
        msg_recv: mpsc::Receiver<()>,
        #[pin]
        error_recv: oneshot::Receiver<JsValue>,
    }
}

impl WasmWebsocket {
    fn new(inner: web_sys::WebSocket) -> Self {
        inner.set_onopen(value);
        Self { inner }
    }
}

impl crate::transport::WebSocket for WasmWebsocket {
    type Error = JsValue;
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Sink<WsMessage> for WasmWebsocket {
    type Error = JsValue;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: WsMessage) -> Result<(), Self::Error> {
        self.project().inner.start_send(item.into())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }
}

impl Stream for WasmWebsocket {
    type Item = Result<WsMessage, JsValue>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.project().inner.poll_next(cx)) {
            Some(Ok(Message::Text(v))) => Poll::Ready(Some(Ok(WsMessage::Text(unsafe {
                Str::from_bytes_unchecked(v.into())
            })))),
            Some(Ok(Message::Binary(v))) => Poll::Ready(Some(Ok(WsMessage::Binary(v)))),
            Some(Ok(Message::Close(_))) => Poll::Ready(Some(Ok(WsMessage::Close))),
            Some(Ok(_)) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Pending,
        }
    }
}
