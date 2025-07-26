use std::{
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use bytes::Bytes;
use engineioxide_core::{Packet, PacketBuf, Sid, Str, TransportType};
use futures_core::Stream;
use http::Request;
use http_body_util::Full;
use hyper::service::Service as HyperSvc;
use smallvec::smallvec;
use tokio::sync::mpsc;

use crate::HttpClient;

pub struct Client<S> {
    pub transport: TransportType,
    pub sid: Sid,
    tx: mpsc::Sender<PacketBuf>,
    pub(crate) rx: Mutex<mpsc::Receiver<PacketBuf>>,
    inner: HttpClient<S>,
}

pub struct ClientStream {}

impl Stream for ClientStream {
    type Item = Packet;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
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
    pub async fn connect(svc: S) -> Result<(Self, ClientStream), ()> {
        let (tx, rx) = mpsc::channel(255);
        let inner = HttpClient::new(svc);
        let packet = inner.handshake().await.unwrap();

        let client = Client {
            transport: TransportType::Polling,
            sid: packet.sid,
            tx,
            rx: Mutex::new(rx),
            inner,
        };

        let stream = ClientStream {};

        Ok((client, stream))
    }

    pub fn emit(&self, msg: Str) {
        self.tx.try_send(smallvec![Packet::Message(msg)]).unwrap();
    }
    pub fn emit_binary(&self, data: Bytes) {
        self.tx.try_send(smallvec![Packet::Binary(data)]).unwrap();
    }

    pub fn connected(&self) -> bool {
        self.rx.try_lock().is_ok()
    }
}
