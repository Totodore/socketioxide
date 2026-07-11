use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use engineioxide_core::{Packet, PacketParseError, Sid};
use futures_core::Stream;
use futures_util::{
    Sink, StreamExt,
    stream::{SplitSink, SplitStream},
};

use crate::{
    HttpClient,
    transport::{Transport, polling::PollingSvc},
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError<S: PollingSvc> {
    #[error("packet parse error")]
    PacketParse(#[from] PacketParseError),
    #[error("transport error")]
    Transport(S::Error),
}

pin_project_lite::pin_project! {
    pub struct Client<S: PollingSvc> {
        #[pin]
        pub transport_rx: SplitStream<Transport<S>>,
        #[pin]
        pub transport_tx: SplitSink<Transport<S>, Packet>,

        should_send_pong: bool,
        should_flush: bool,
        pub sid: Sid,
    }
}

impl<S: PollingSvc> Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    pub async fn connect(svc: S) -> Result<Self, PacketParseError> {
        let (inner, open) = HttpClient::connect(svc).await?;
        let transport = Transport::Polling { inner };
        let (transport_tx, transport_rx) = transport.split();
        let client = Client {
            transport_tx,
            transport_rx,
            sid: open.sid,

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
        if let Err(e) = ready!(this.transport_tx.as_mut().poll_ready(cx)) {
            return Poll::Ready(Err(e));
        }
        if let Err(e) = this.transport_tx.as_mut().start_send(Packet::Pong) {
            return Poll::Ready(Err(e));
        }

        *this.should_send_pong = false;
        *this.should_flush = true;
        Poll::Ready(Ok(()))
    }

    #[tracing::instrument(skip(cx))]
    fn flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        let mut this = self.project();
        if let Err(e) = ready!(this.transport_tx.as_mut().poll_flush(cx)) {
            return Poll::Ready(Err(e));
        }
        *this.should_flush = false;
        Poll::Ready(Ok(()))
    }
}

impl<S: PollingSvc> Stream for Client<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Item = Result<Packet, ClientError<S>>;

    #[tracing::instrument(skip(cx))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.should_send_pong {
            //TODO: ret err
            if let Err(e) = ready!(self.as_mut().heartbeat(cx)) {
                return Poll::Ready(Some(Err(ClientError::Transport(e))));
            }
        }

        if self.should_flush {
            //TODO: ret err
            if let Err(e) = ready!(self.as_mut().flush(cx)) {
                return Poll::Ready(Some(Err(ClientError::Transport(e))));
            }
        }

        match ready!(self.as_mut().project().transport_rx.poll_next(cx)) {
            Some(Ok(Packet::Ping)) => {
                if self.as_mut().heartbeat(cx).is_pending() {
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
    type Error = <Transport<S> as Sink<Packet>>::Error;

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
