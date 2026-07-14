pub mod hyper;
pub mod hyper_tungstenite;
pub mod testing;
pub mod wasm;

mod noop_impl {
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures_core::Stream;
    use futures_util::Sink;

    use crate::transport::ws::{WebSocket, WsMessage};

    #[derive(Debug, Default, Clone)]
    pub struct NoopWebSocket;
    impl WebSocket for NoopWebSocket {
        type Error = Infallible;
    }

    impl Stream for NoopWebSocket {
        type Item = Result<WsMessage, Infallible>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(None)
        }
    }

    impl Sink<WsMessage> for NoopWebSocket {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _item: WsMessage) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}
