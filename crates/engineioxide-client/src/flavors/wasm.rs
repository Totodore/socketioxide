use std::{convert::Infallible, future::Ready};

use bytes::Bytes;
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::{
    Client, ResponseFuture,
    connect::{HttpConnector, dns::GaiResolver},
};
use web_sys::RequestInit;

use crate::{flavors::noop_impl::NoopWebSocket, transport::WebSocket};

#[derive(Debug, Clone)]
pub struct WasmFlavor {
    // client: Client<HttpConnector<GaiResolver>, BoxBody<Bytes, Infallible>>,
}

impl WasmFlavor {
    pub fn new() -> Self {
        // Self {
        // client: Client::builder(hyper_util::rt::TokioExecutor::new()).build_http(),
        // }
    }
}
impl Default for WasmFlavor {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP Service implementation
impl hyper::service::Service<http::Request<BoxBody<Bytes, Infallible>>> for WasmFlavor {
    type Response = Response<Incoming>;
    type Error = hyper_util::client::legacy::Error;
    type Future = ResponseFuture;

    fn call(&self, req: http::Request<BoxBody<Bytes, Infallible>>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let uri = req.uri().to_string();
        let opts = RequestInit::new();
        //TODO: machinery for box body collection here
        //TODO: machinery for body response
        //TODO: headermap to jsvalue
        opts.set_method(parts.method.as_str());
        opts.set_headers(val);

        web_sys::window()
            .unwrap()
            .fetch_with_str_and_init(&uri, &opts);

        todo!()
    }
}

// /// WS Service Implementation
// impl hyper::service::Service<http::Request<()>> for WasmFlavor {
//     type Response = WasmWebsocket<MaybeTlsStream<TcpStream>>;
//     type Error = tungstenite::Error;
//     type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

//     fn call(&self, req: http::Request<()>) -> Self::Future {
//         async move {
//             let (ws, _) = tokio_tungstenite::connect_async(req).await?;
//             Ok(ws.into())
//         }
//         .boxed()
//     }
// }

// pin_project! {
//     pub struct WasmWebsocket<S> {
//         #[pin]
//         inner: web_sys::,
//     }
// }

// impl<S> From<tokio_tungstenite::WebSocketStream<S>> for WasmWebsocket<S>
// where
//     S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
// {
//     fn from(inner: tokio_tungstenite::WebSocketStream<S>) -> Self {
//         Self { inner }
//     }
// }

// impl<S> WebSocket for WasmWebsocket<S>
// where
//     S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
// {
//     type Error = tungstenite::Error;
// }

// impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Sink<WsMessage> for WasmWebsocket<S> {
//     type Error = tungstenite::Error;

//     fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.project().inner.poll_ready(cx)
//     }

//     fn start_send(self: Pin<&mut Self>, item: WsMessage) -> Result<(), Self::Error> {
//         self.project().inner.start_send(item.into())
//     }

//     fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.project().inner.poll_flush(cx)
//     }

//     fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.project().inner.poll_close(cx)
//     }
// }

// impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Stream for WasmWebsocket<S> {
//     type Item = Result<WsMessage, tungstenite::Error>;

//     fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         match ready!(self.project().inner.poll_next(cx)) {
//             Some(Ok(Message::Text(v))) => Poll::Ready(Some(Ok(WsMessage::Text(unsafe {
//                 Str::from_bytes_unchecked(v.into())
//             })))),
//             Some(Ok(Message::Binary(v))) => Poll::Ready(Some(Ok(WsMessage::Binary(v)))),
//             Some(Ok(Message::Close(_))) => Poll::Ready(Some(Ok(WsMessage::Close))),
//             Some(Ok(_)) => {
//                 cx.waker().wake_by_ref();
//                 Poll::Pending
//             }
//             Some(Err(e)) => Poll::Ready(Some(Err(e))),
//             None => Poll::Pending,
//         }
//     }
// }
