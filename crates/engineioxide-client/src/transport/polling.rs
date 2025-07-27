use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use engineioxide_core::OpenPacket;
use engineioxide_core::Packet;
use engineioxide_core::PacketBuf;
use engineioxide_core::PacketParseError;
use engineioxide_core::ProtocolVersion;
use engineioxide_core::Sid;
use engineioxide_core::payload;
use futures_core::Stream;
use futures_util::FutureExt;
use futures_util::Sink;
use futures_util::StreamExt;
use futures_util::TryStreamExt;
use http::Request;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::service::Service as HyperSvc;

use crate::poll;

pin_project_lite::pin_project! {
    #[project = PollStateProj]
    enum PollState<F> {
        No,
        Pending {
            #[pin]
            fut: F
        },
        Decoding {
            stream: Pin<Box<dyn Stream<Item = Result<Packet, PacketParseError>>>>
        }
    }
}
pin_project_lite::pin_project! {
    pub struct HttpClient<S>
    where
        S: HyperSvc<Request<Full<Bytes>>>,
    {
        svc: S,
        poll_state: PollState<S::Future>,
    }
}

impl<S> HttpClient<S>
where
    S: HyperSvc<Request<Full<Bytes>>>,
    S::Response: hyper::body::Body,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug,
    <S::Response as hyper::body::Body>::Data: std::fmt::Debug,
    S::Error: std::fmt::Debug,
{
    pub fn new(svc: S) -> Self {
        Self {
            svc,
            poll_state: PollState::No,
        }
    }
    pub async fn handshake(&self) -> Result<OpenPacket, PacketParseError> {
        let req = Request::builder()
            .method("GET")
            .uri("http://localhost:3000/engine.io?EIO=4&transport=polling")
            .body(Full::default())
            .unwrap();
        let res = self.svc.call(req).await;
        let body = res.unwrap().collect().await.unwrap();
        let packet = Packet::try_from(String::from_utf8(body.to_bytes().to_vec()).unwrap())?;
        match packet {
            Packet::Open(open) => Ok(open),
            _ => Err(PacketParseError::InvalidPacketType(Some('1'))),
        }
    }

    pub async fn post(&self, id: Sid, packet: impl Into<Bytes>) -> Result<(), S::Error> {
        let uri = format!("http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}");
        let req = Request::post(uri).body(Full::from(packet.into())).unwrap();
        self.svc.call(req).await?;
        Ok(())
    }
}

impl<S> Sink<PacketBuf> for HttpClient<S> {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {}

    fn start_send(self: Pin<&mut Self>, item: PacketBuf) -> Result<(), Self::Error> {}

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {}

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {}
}

impl<S> Stream for HttpClient<S>
where
    S: HyperSvc<Request<Full<Bytes>>>,
    S::Response: hyper::body::Body + 'static,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug + 'static,
    <S::Response as hyper::body::Body>::Data: Send + std::fmt::Debug + 'static,
    S::Error: std::fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().poll_state {
            PollState::No => {
                let id = Sid::new();
                let uri =
                    format!("http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}");
                let req = Request::get(uri).body(Full::new(Bytes::new())).unwrap();
                let fut = self.svc.call(req);
                self.poll_state = PollState::Pending { fut };
                Poll::Pending
            }
            PollState::Pending { ref mut fut } => {
                match poll!(unsafe { Pin::new_unchecked(fut) }.poll(cx)) {
                    Ok(body) => {
                        let body = Box::pin(body);
                        let stream =
                            payload::decoder(body, None, ProtocolVersion::V4, 200).boxed_local();
                        self.poll_state = PollState::Decoding { stream };
                        Poll::Pending
                    }
                    Err(err) => todo!(),
                }
            }
            PollState::Decoding { ref mut stream } => match poll!(stream.poll_next_unpin(cx)) {
                Some(packet) => Poll::Ready(Some(packet)),
                None => {
                    self.poll_state = PollState::No;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            },
        }
    }
}

impl<F> PollState<F> {
    fn get_decoding(self) -> Pin<Box<dyn Stream<Item = Result<Packet, PacketParseError>>>> {
        match self {
            PollState::Decoding { stream } => stream,
            _ => unreachable!(),
        }
    }
}
