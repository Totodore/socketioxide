use std::{
    convert::Infallible,
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{BufMut, Bytes, BytesMut};
use engineioxide_core::{OpenPacket, Packet, PacketParseError, ProtocolVersion, Sid, payload};
use futures_core::Stream;
use futures_util::{Sink, StreamExt};
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;

pub trait PollingSvc:
    HyperSvc<
        Request<BoxBody<Bytes, Infallible>>,
        Response = Response<Self::Body>,
        Error = <Self as PollingSvc>::Error,
    >
{
    type Body: http_body::Body<Error = Self::ResBodyError> + 'static;
    type Error: fmt::Debug + std::error::Error;
    type ResBodyError: fmt::Debug + std::error::Error + 'static;
}

impl<B, S> PollingSvc for S
where
    S: HyperSvc<Request<BoxBody<Bytes, Infallible>>, Response = Response<B>>,
    <S as HyperSvc<Request<BoxBody<Bytes, Infallible>>>>::Error: fmt::Debug + std::error::Error,
    B: http_body::Body + 'static,
    <B as http_body::Body>::Error: fmt::Debug + std::error::Error + 'static,
    <B as http_body::Body>::Data: Send + fmt::Debug + 'static,
{
    type Body = B;
    type Error = <S as HyperSvc<Request<BoxBody<Bytes, Infallible>>>>::Error;
    type ResBodyError = <B as http_body::Body>::Error;
}

pin_project! {
    #[project = PollStateProj]
    enum PollState<F> {
        Pending {
            #[pin]
            fut: F
        },
        Decoding {
            #[pin]
            stream: Pin<Box<dyn Stream<Item = Result<Packet, PacketParseError>>>>
        }
    }
}

pin_project! {
    #[project = PostStateProj]
    enum PostState<F> {
        Queuing {
            bytes: BytesMut,
        },
        Pending {
            #[pin]
            fut: F,
            // TODO: BytesList
            bytes: BytesMut,
        }
    }
}

impl<F> Default for PostState<F> {
    fn default() -> Self {
        PostState::Queuing {
            bytes: BytesMut::new(),
        }
    }
}

impl<F> PollState<F> {
    fn new_request<S: PollingSvc<Future = F>>(svc: &S, sid: Sid) -> Self {
        let uri = format!("http://localhost:3000/engine.io?EIO=4&transport=polling&sid={sid}");
        let req = Request::get(uri)
            .body(BoxBody::new(Full::default()))
            .unwrap();
        let fut = svc.call(req);
        PollState::Pending { fut }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PollingTransportError<S: PollingSvc> {
    #[error("polling error: {0}")]
    Polling(<S as PollingSvc>::Error),
    #[error("polling body error: {0}")]
    PollingBody(<S as PollingSvc>::ResBodyError),
    #[error("packet error: {0}")]
    Packet(#[from] PacketParseError),
}

pin_project! {
    pub struct PollingTransport<S: PollingSvc>
    {
        pub(crate) svc: S,

        #[pin]
        poll_state: PollState<S::Future>,

        #[pin]
        post_state: PostState<S::Future>,

        sid: Sid,
    }
}

impl<S: PollingSvc> PollingTransport<S> {
    pub async fn connect(svc: S) -> Result<(Self, OpenPacket), PollingTransportError<S>> {
        tracing::trace!("handshake request");

        let req = Request::builder()
            .method("GET")
            .uri("http://localhost:3000/engine.io?EIO=4&transport=polling")
            .body(BoxBody::new(Full::default()))
            .unwrap();

        let res = svc
            .call(req)
            .await
            .map_err(PollingTransportError::Polling)?;
        let body = res
            .collect()
            .await
            .map_err(PollingTransportError::PollingBody)?;

        let packet = Packet::parse(
            ProtocolVersion::V4,
            String::from_utf8(body.to_bytes().to_vec()).unwrap(),
        )?;

        match packet {
            Packet::Open(open) => {
                let poll_state = PollState::new_request(&svc, open.sid);
                let transport = PollingTransport {
                    svc,
                    poll_state,
                    post_state: PostState::default(),
                    sid: open.sid,
                };

                tracing::debug!(?transport, ?open, "polling transport intialized");
                Ok((transport, open))
            }
            _ => Err(PollingTransportError::Packet(
                PacketParseError::InvalidPacketType(None),
            )),
        }
    }
}

impl<S: PollingSvc> Stream for PollingTransport<S> {
    type Item = Result<Packet, PollingTransportError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        tracing::trace!(poll_state = ?self.poll_state, "polling");

        let mut proj = self.as_mut().project().poll_state.project();
        match proj {
            PollStateProj::Pending { ref mut fut } => {
                match ready!(fut.as_mut().poll(cx)) {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
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
                        tracing::debug!(?err, "got body error");
                        Poll::Ready(Some(Err(PacketParseError::InvalidPacketPayload.into())))
                    }
                }
            }
            PollStateProj::Decoding { stream } => {
                if let Some(packet) = ready!(stream.poll_next(cx)) {
                    Poll::Ready(Some(packet.map_err(PollingTransportError::from)))
                } else {
                    tracing::debug!(
                        sid = %self.sid,
                        "decoding stream ended, new polling req"
                    );
                    let request = PollState::new_request(&self.svc, self.sid);
                    self.project().poll_state.set(request);
                    //check if wake is needed
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }
    }
}

impl<S: PollingSvc> Sink<Packet> for PollingTransport<S> {
    type Error = PollingTransportError<S>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        tracing::trace!(post_state = ?self.post_state, "sending packet");
        self.project().post_state.encode(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        tracing::trace!(post_state = ?self.post_state, "flushing");
        let proj = self.as_mut().project().post_state.project();

        match proj {
            PostStateProj::Queuing { bytes } if bytes.is_empty() => Poll::Ready(Ok(())),
            PostStateProj::Queuing { bytes } => {
                let body = std::mem::take(bytes).freeze();
                //TODO: handle max body size from open packet
                let sid = self.sid;
                let req = Request::post(format!(
                    "http://localhost:3000/engine.io?EIO=4&transport=polling&sid={sid}",
                ))
                .body(BoxBody::new(Full::new(body)))
                .unwrap();

                let fut = self.svc.call(req);
                self.project().post_state.set(PostState::Pending {
                    fut,
                    bytes: BytesMut::new(),
                });
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PostStateProj::Pending { fut, .. } => {
                match ready!(fut.poll(cx)) {
                    Ok(res) => {
                        assert!(res.status().is_success());
                        self.project().post_state.set(PostState::default());
                        Poll::Ready(Ok(())) // TODO: check response == ok
                    }
                    Err(err) => {
                        self.project().post_state.set(PostState::default());
                        Poll::Ready(Err(PollingTransportError::Polling(err)))
                    }
                }
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        //TODO: close behavior
        Poll::Ready(Ok(()))
    }
}

impl<F> PostState<F> {
    pub fn encode(self: Pin<&mut Self>, item: Packet) {
        const PACKET_SEPARATOR_V4: u8 = b'\x1e';
        let packet: Bytes = item.into();
        let bytes = match self.project() {
            PostStateProj::Queuing { bytes } => bytes,
            PostStateProj::Pending { bytes, .. } => bytes,
        };

        if !bytes.is_empty() {
            bytes.put_u8(PACKET_SEPARATOR_V4);
        }
        bytes.extend_from_slice(&packet);
    }
}

impl<F> fmt::Debug for PollState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending { .. } => write!(f, "Pending"),
            Self::Decoding { .. } => write!(f, "Decoding"),
        }
    }
}
impl<F> fmt::Debug for PostState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending { .. } => write!(f, "Pending"),
            Self::Queuing { .. } => write!(f, "Queuing"),
        }
    }
}
impl<S: PollingSvc> fmt::Debug for PollingTransport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollingTransport")
            .field("poll_state", &self.poll_state)
            .field("post_state", &self.post_state)
            .field("sid", &self.sid)
            .finish()
    }
}
