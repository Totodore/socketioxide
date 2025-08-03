use std::fmt;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use bytes::Bytes;
use bytes::BytesMut;
use engineioxide_core::OpenPacket;
use engineioxide_core::Packet;
use engineioxide_core::PacketParseError;
use engineioxide_core::ProtocolVersion;
use engineioxide_core::Sid;
use engineioxide_core::payload;
use futures_core::Stream;
use futures_util::Sink;
use futures_util::StreamExt;
use http::Request;
use http::Response;
use http::StatusCode;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;

use crate::poll;

pub trait PollingSvc: HyperSvc<Request<Full<Bytes>>, Response = Response<Self::Body>> {
    type Body: hyper::body::Body + 'static;
}

impl<B, S> PollingSvc for S
where
    S: HyperSvc<Request<Full<Bytes>>, Response = Response<B>>,
    <S as HyperSvc<Request<Full<Bytes>>>>::Error: fmt::Debug,
    B: hyper::body::Body + 'static,
    <B as hyper::body::Body>::Error: std::fmt::Debug + 'static,
    <B as hyper::body::Body>::Data: Send + std::fmt::Debug + 'static,
{
    type Body = B;
}

pin_project! {
    #[project = PollStateProj]
    #[derive(Default)]
    enum PollState<F> {
        #[default]
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

pin_project! {
    #[project = PostStateProj]
    enum PostState<F> {
        Encoding {
            body: BytesMut
        },
        Pending {
            #[pin]
            fut: F
        }
    }
}

impl<F> Default for PostState<F> {
    fn default() -> Self {
        PostState::Encoding {
            body: BytesMut::default(),
        }
    }
}

pin_project! {
    pub struct HttpClient<S: PollingSvc>
    {
        svc: S,
        poll_state: PollState<S::Future>,
        post_state: PostState<S::Future>,
        sid: Option<Sid>,
    }
}

impl<S: PollingSvc> HttpClient<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    pub fn new(svc: S) -> Self {
        Self {
            svc,
            poll_state: PollState::default(),
            post_state: PostState::default(),
            sid: None,
        }
    }

    pub async fn handshake(&mut self) -> Result<OpenPacket, PacketParseError> {
        #[cfg(feature = "tracing")]
        tracing::trace!(?self, "handshake request");

        let req = Request::builder()
            .method("GET")
            .uri("http://localhost:3000/engine.io?EIO=4&transport=polling")
            .body(Full::default())
            .unwrap();
        let res = self.svc.call(req).await;
        let body = res.unwrap().collect().await.unwrap();
        let packet = Packet::try_from(String::from_utf8(body.to_bytes().to_vec()).unwrap())?;

        match packet {
            Packet::Open(open) => {
                self.sid = Some(open.sid);
                Ok(open)
            }
            _ => Err(PacketParseError::InvalidPacketType(Some('1'))),
        }
    }
}

impl<S: PollingSvc> Stream for HttpClient<S>
where
    S::Error: fmt::Debug,
    <S::Body as http_body::Body>::Error: fmt::Debug,
{
    type Item = Result<Packet, PacketParseError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        #[cfg(feature = "tracing")]
        tracing::trace!(poll_state = ?self.poll_state, "polling");

        match self.as_mut().poll_state {
            PollState::No => {
                let id = self.sid.unwrap();
                let uri =
                    format!("http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}");
                let req = Request::get(uri).body(Full::new(Bytes::new())).unwrap();
                let fut = self.svc.call(req);
                self.poll_state = PollState::Pending { fut };
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PollState::Pending { ref mut fut } => {
                match poll!(unsafe { Pin::new_unchecked(fut) }.poll(cx)) {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
                        dbg!(&parts);
                        assert!(parts.status == StatusCode::OK);
                        let body = Box::pin(body);
                        //TODO: implement limited body + Content-Type
                        let stream =
                            payload::decoder(body, None, ProtocolVersion::V4, 200).boxed_local();
                        self.poll_state = PollState::Decoding { stream };
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Err(err) => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(?err, "got body error");
                        Poll::Ready(Some(Err(PacketParseError::InvalidPacketPayload)))
                    }
                }
            }
            PollState::Decoding { ref mut stream } => {
                if let Some(packet) = poll!(stream.poll_next_unpin(cx)) {
                    Poll::Ready(Some(packet))
                } else {
                    self.poll_state = PollState::No;
                    // Should not be needed.
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }
    }
}

impl<S: PollingSvc> Sink<Packet> for HttpClient<S> {
    type Error = ();

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.post_state {
            PostState::Encoding { .. } => Poll::Ready(Ok(())),
            _ => Poll::Pending,
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        #[cfg(feature = "tracing")]
        tracing::trace!(post_state = ?self.post_state, "sending packet");

        let body = match &self.post_state {
            PostState::Encoding { body } => body,
            _ => panic!(
                "unexpected state, Sink::poll_ready should always be called before Sink::start_send"
            ),
        };
        //TODO: write packet

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        #[cfg(feature = "tracing")]
        tracing::trace!(post_state = ?self.post_state, "flushing");

        match self.post_state {
            PostState::Encoding { ref body } => {
                let req = Request::post(
                    "http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}",
                )
                .body(Full::new(body.clone().freeze())) //TODO: fix cloning
                .unwrap();
                let fut = self.svc.call(req);
                self.post_state = PostState::Pending { fut };
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PostState::Pending { ref mut fut } => {
                match poll!(unsafe { Pin::new_unchecked(fut) }.poll(cx)) {
                    Ok(res) => {
                        self.post_state = PostState::default();
                        Poll::Ready(Ok(())) // TODO: check response == ok
                    }
                    Err(err) => {
                        self.post_state = PostState::default();
                        todo!("handle error")
                    }
                }
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
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

impl<F> fmt::Debug for PollState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::No => write!(f, "No"),
            Self::Pending { .. } => write!(f, "Pending"),
            Self::Decoding { .. } => write!(f, "Decoding"),
        }
    }
}
impl<F> fmt::Debug for PostState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encoding { body } => f
                .debug_struct("Encoding")
                .field("body_len", &body.len())
                .finish(),
            Self::Pending { .. } => write!(f, "Pending"),
        }
    }
}
impl<S: PollingSvc> fmt::Debug for HttpClient<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("poll_state", &self.poll_state)
            .field("post_state", &self.post_state)
            .field("sid", &self.sid)
            .finish()
    }
}
