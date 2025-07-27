use std::{
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use bytes::Bytes;
use engineioxide_core::{PacketBuf, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use http::Request;
use http_body_util::Full;
use hyper::service::Service as HyperSvc;
use tokio::sync::mpsc::{self, error::TrySendError};

use crate::{HttpClient, transport::Transport};

pin_project_lite::pin_project! {
    pub struct Client<S> {
        pub transport: Transport<S>,
        pub sid: Sid,
        pub tx: mpsc::Sender<PacketBuf>,
        pub(crate) rx: Mutex<mpsc::Receiver<PacketBuf>>,

        #[pin]
        rrx: mpsc::Receiver<PacketBuf>,
    }
}

impl<S> Client<S>
where
    S: HyperSvc<Request<Full<Bytes>>>,
    S::Response: hyper::body::Body,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug,
    <S::Response as hyper::body::Body>::Data: std::fmt::Debug,
    S::Error: std::fmt::Debug,
{
    pub async fn connect(svc: S) -> Result<Self, ()> {
        let (tx, rx) = mpsc::channel(255);
        let inner = HttpClient::new(svc);
        let packet = inner.handshake().await.unwrap();

        let (rtx, rrx) = mpsc::channel(100);
        let client = Client {
            transport: Transport::Polling(inner),
            sid: packet.sid,
            tx,
            rx: Mutex::new(rx),
            rrx,
        };

        Ok(client)
    }

    pub fn connected(&self) -> bool {
        self.rx.try_lock().is_ok()
    }

    pub fn transport_type(&self) -> TransportType {
        self.transport.transport_type()
    }
}

impl<S> Sink<PacketBuf> for Client<S> {
    type Error = TrySendError<PacketBuf>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, packets: PacketBuf) -> Result<(), Self::Error> {
        self.tx.try_send(packets)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl<S> Stream for Client<S> {
    type Item = PacketBuf;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rrx.poll_recv(cx)
    }
}
