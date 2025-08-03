use std::{
    pin::Pin,
    task::{Context, Poll},
};

use engineioxide_core::{Packet, PacketParseError, TransportType};
use futures_core::Stream;

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
    S::Response: hyper::body::Body + 'static,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug + 'static,
    <S::Response as hyper::body::Body>::Data: Send + std::fmt::Debug + 'static,
    S::Error: std::fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project() {
            TransportProj::Polling { inner } => inner.poll_next(cx),
            TransportProj::Websocket { inner } => inner.poll_next(cx),
        }
    }
}
