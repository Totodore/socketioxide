use std::collections::VecDeque;
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
        /// Ideally the queue should gradually encode packets in an async fashion
        Queuing {
            queue: VecDeque<Packet>
        },
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
        #[pin]
        poll_state: PollState<S::Future>,
        #[pin]
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

        let mut poll_state_proj = self.as_mut().project().poll_state.project();
        match poll_state_proj {
            PollStateProj::No => {
                let id = self.sid.unwrap();
                let uri =
                    format!("http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}");
                let req = Request::get(uri).body(Full::new(Bytes::new())).unwrap();
                let fut = self.svc.call(req);
                self.project().poll_state.set(PollState::Pending { fut });
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PollStateProj::Pending { ref mut fut } => {
                match poll!(fut.as_mut().poll(cx)) {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
                        dbg!(&parts);
                        assert!(parts.status == StatusCode::OK);
                        let body = Box::pin(body);
                        //TODO: implement limited body + Content-Type
                        let stream =
                            payload::decoder(body, None, ProtocolVersion::V4, 200).boxed_local();

                        self.project()
                            .poll_state
                            .set(PollState::Decoding { stream });

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
            PollStateProj::Decoding { ref mut stream } => {
                if let Some(packet) = poll!(stream.poll_next_unpin(cx)) {
                    dbg!(&packet);
                    Poll::Ready(Some(packet))
                } else {
                    self.project().poll_state.set(PollState::No);
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

        self.project().post_state.set(PostState::Encoding { body });

        //TODO: write packet

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        #[cfg(feature = "tracing")]
        tracing::trace!(post_state = ?self.post_state, "flushing");

        match self.as_mut().project().post_state.project() {
            PostStateProj::Queuing { queue } => {
                // payload::encoder(rx, protocol, supports_binary, max_payload)
                if let Some(packet) = packet {
                    self.project().post_state.set(PostState::Encoding { body });
                }
                Poll::Ready(Ok(()))
            }
            PostStateProj::Encoding { body } => {
                let req = Request::post(
                    "http://localhost:3000/engine.io?EIO=4&transport=polling&sid={id}",
                )
                .body(Full::new(body.clone())) //TODO: fix cloning
                .unwrap();
                let fut = self.svc.call(req);
                self.project().post_state.set(PostState::Pending { fut });
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PostStateProj::Pending { fut } => {
                match poll!(fut.poll(cx)) {
                    Ok(res) => {
                        assert!(res.status().is_success());
                        self.project().post_state.set(PostState::default());
                        Poll::Ready(Ok(())) // TODO: check response == ok
                    }
                    Err(err) => {
                        self.project().post_state.set(PostState::default());
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
