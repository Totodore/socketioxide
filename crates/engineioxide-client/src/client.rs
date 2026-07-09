use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, PacketParseError, Sid};
use futures_core::Stream;
use futures_util::{
    Sink, SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};

use crate::{
    HttpClient,
    transport::{Transport, polling::PollingSvc},
};

type SendPongFut<S> = Pin<
    Box<
        dyn Future<Output = Result<(), <SplitSink<Transport<S>, Packet> as Sink<Packet>>::Error>>
            + 'static,
    >,
>;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("packet parse error")]
    PacketParse(#[from] PacketParseError),
    #[error("transport error")]
    Transport(#[from] Box<dyn std::error::Error>),
}

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc> {
        #[pin]
        pub transport_rx: SplitStream<Transport<S>>,
        // TODO: is this the right implementation? We need something that can be driven itself.
        // Otherwise we need a way to drive the transport_tx. Normally it should be driven by the user.
        // But what if we need to send a PONG packet from the inner lib?
        #[pin]
        pub transport_tx: SplitSink<Transport<S>, Packet>,

        should_send_pong: bool,
        should_flush: bool,
        pub sid: Sid,
        // pub tx: mpsc::Sender<PacketBuf>,
        // pub(crate) rx: Mutex<mpsc::Receiver<PacketBuf>>,
    }
}

impl<S: PollingSvc> Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    pub async fn connect(svc: S) -> Result<Self, PacketParseError> {
        let mut inner = HttpClient::new(svc);
        let packet = inner.handshake().await?;

        let transport = Transport::Polling { inner };
        let (transport_tx, transport_rx) = transport.split();
        let client = Client {
            transport_tx,
            transport_rx,
            sid: packet.sid,

            should_flush: false,
            should_send_pong: false,
        };

        Ok(client)
    }
}
impl<S: PollingSvc> Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    #[tracing::instrument(skip(cx))]
    fn heartbeat(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        let mut this = self.project();
        ready!(this.transport_tx.as_mut().poll_ready(cx));
        let res = this.transport_tx.as_mut().start_send(Packet::Pong);
        *this.should_send_pong = false;
        *this.should_flush = true;
        Poll::Ready(Ok(res.unwrap()))
    }

    #[tracing::instrument(skip(cx))]
    fn flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        let mut this = self.project();
        ready!(this.transport_tx.as_mut().poll_flush(cx));
        *this.should_flush = false;
        Poll::Ready(Ok(()))
    }
}

impl<S: PollingSvc> Stream for Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Item = Result<Packet, ClientError>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.should_send_pong {
            //TODO: ret err
            self.as_mut().heartbeat(cx);
        }

        if self.should_flush {
            //TODO: ret err
            self.as_mut().flush(cx);
        }

        match ready!(self.as_mut().project().transport_rx.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                dbg!("got ping packet");
                if let Poll::Pending = self.as_mut().heartbeat(cx) {
                    self.should_send_pong = true;
                }
                self.poll_next(cx)
            }
            Some(Ok(packet)) => Poll::Ready(Some(Ok(packet))),
            Some(Err(e)) => Poll::Ready(Some(Err(e.into()))),
            None => Poll::Ready(None),
        }
    }
}

impl<S: PollingSvc> Sink<Packet> for Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        self.project().transport_tx.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().transport_tx.poll_close(cx)
    }
}

impl<S: PollingSvc> fmt::Debug for Client<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("should_send_pong", &self.should_send_pong)
            .field("should_flush", &self.should_flush)
            .field("sid", &self.sid)
            .finish()
    }
}
