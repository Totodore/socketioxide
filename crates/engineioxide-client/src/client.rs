use std::{
    fmt,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use engineioxide_core::{Packet, PacketBuf, PacketParseError, Sid};
use futures_core::Stream;
use futures_util::{
    Sink, SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use tokio::sync::mpsc::{self, error::TrySendError};

use crate::{
    HttpClient, poll,
    transport::{Transport, polling::PollingSvc},
};

type SendPongFut<S> = Pin<
    Box<
        dyn Future<Output = Result<(), <SplitSink<Transport<S>, Packet> as Sink<Packet>>::Error>>
            + 'static,
    >,
>;

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc> {
        #[pin]
        pub transport_rx: SplitStream<Transport<S>>,
        // TODO: is this the right implementation? We need something that can be driven itself.
        // Otherwise we need a way to drive the transport_tx. Normally it should be driven by the user.
        // But what if we need to send a PONG packet from the inner lib?
        #[pin]
        pub transport_tx: SplitSink<Transport<S>, Packet>,

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
    pub async fn connect(svc: S) -> Result<Self, ()> {
        let mut inner = HttpClient::new(svc);
        let packet = inner.handshake().await.unwrap();

        let transport = Transport::Polling { inner };
        let (transport_tx, transport_rx) = transport.split();
        let client = Client {
            transport_tx,
            transport_rx,
            sid: packet.sid,
        };

        Ok(client)
    }
}

impl<S: PollingSvc> Stream for Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match poll!(this.transport_rx.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                cx.waker().wake_by_ref();
                // let mut tx = self.transport_tx.clone();
                // let fut = async move {
                //     tx.send(Packet::Pong).await?;
                //     tx.flush().await
                // };
                // this.pending_pong.set(Some(Box::pin(fut)));

                Poll::Pending
            }
            packet => Poll::Ready(packet),
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
