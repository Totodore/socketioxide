use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use engineioxide_core::{Packet, PacketParseError, TransportType};
use futures_core::Stream;
use futures_util::Sink;

use crate::{HttpClient, transport::polling::PollingSvc};

pub mod polling;

pin_project_lite::pin_project! {
    #[project = TransportProj]
    pub enum Transport<S: PollingSvc> {
        Polling {
            #[pin]
            inner: HttpClient<S>
        },
        Websocket {
            #[pin]
            inner: HttpClient<S>
        }
    }
}

impl<S: PollingSvc> Transport<S> {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Transport::Polling { .. } => TransportType::Polling,
            Transport::Websocket { .. } => TransportType::Websocket,
        }
    }
}

impl<S: PollingSvc> Stream for Transport<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project() {
            TransportProj::Polling { inner } => inner.poll_next(cx),
            TransportProj::Websocket { inner } => inner.poll_next(cx),
        }
    }
}
impl<S: PollingSvc> Sink<Packet> for Transport<S> {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_ready(cx),
            TransportProj::Websocket { inner } => inner.poll_ready(cx),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        match self.project() {
            TransportProj::Polling { inner } => inner.start_send(item),
            TransportProj::Websocket { inner } => inner.start_send(item),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_flush(cx),
            TransportProj::Websocket { inner } => inner.poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            TransportProj::Polling { inner } => inner.poll_close(cx),
            TransportProj::Websocket { inner } => inner.poll_close(cx),
        }
    }
}
