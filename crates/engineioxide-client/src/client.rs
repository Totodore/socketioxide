use std::{
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use engineioxide_core::{Packet, PacketBuf, PacketParseError, Sid, TransportType};
use futures_core::Stream;
use futures_util::Sink;
use tokio::sync::mpsc::{self, error::TrySendError};

use crate::{
    HttpClient,
    transport::{Transport, polling::PollingSvc},
};

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc> {
        #[pin]
        pub transport: Transport<S>,
        pub sid: Sid,
        pub tx: mpsc::Sender<PacketBuf>,
        pub(crate) rx: Mutex<mpsc::Receiver<PacketBuf>>,
    }
}

impl<S: PollingSvc> Client<S>
where
    S::Response: hyper::body::Body,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug,
    <S::Response as hyper::body::Body>::Data: std::fmt::Debug,
    S::Error: std::fmt::Debug,
{
    pub async fn connect(svc: S) -> Result<Self, ()> {
        let (tx, rx) = mpsc::channel(255);
        let inner = HttpClient::new(svc);
        let packet = inner.handshake().await.unwrap();

        let client = Client {
            transport: Transport::Polling { inner },
            sid: packet.sid,
            tx,
            rx: Mutex::new(rx),
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

impl<S: PollingSvc> Sink<PacketBuf> for Client<S> {
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

impl<S: PollingSvc> Stream for Client<S>
where
    S::Response: hyper::body::Body + 'static,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug + 'static,
    <S::Response as hyper::body::Body>::Data: Send + std::fmt::Debug + 'static,
    S::Error: std::fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().transport.poll_next(cx)
    }
}
