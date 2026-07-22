use std::{
    convert::Infallible,
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use engineioxide_core::{
    OpenPacket, Packet, PacketParseError, ProtocolVersion, Sid, TransportType, payload,
};
use futures_core::Stream;
use futures_util::{FutureExt, Sink, StreamExt};
use http::{Request, Response, StatusCode, Uri, response};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::service::Service as HyperSvc;
use pin_project_lite::pin_project;
use serde::Deserialize;

use crate::EngineIoClientConfig;

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
        },
        // Terminal state: the previous request future is dropped so it can
        // never be polled again after it completed with an error.
        Closed,
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
        },
        // Terminal state: in-flight request and queued bytes are discarded.
        Closed,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum ClosingState {
    #[default]
    Open,
    Closing,
    Closed,
}
impl<F> Default for PostState<F> {
    fn default() -> Self {
        PostState::Queuing {
            bytes: BytesMut::new(),
        }
    }
}
impl<F> PostState<F> {
    fn queuing(bytes: BytesMut) -> Self {
        Self::Queuing { bytes }
    }
}

impl<F> PollState<F> {
    fn new_request<S: PollingSvc<Future = F>>(svc: &S, base_uri: &Uri, sid: Sid) -> Self {
        let uri = super::with_mandatory_query(base_uri, TransportType::Polling, Some(sid));

        let req = Request::builder()
            .method(http::Method::GET)
            .uri(uri)
            .body(BoxBody::new(Empty::new()))
            .unwrap();

        let fut = svc.call(req);
        PollState::Pending { fut }
    }
}
impl<F> PostState<F> {
    fn new_request<S: PollingSvc<Future = F>>(
        svc: &S,
        uri: &Uri,
        sid: Sid,
        body: BytesMut,
    ) -> Self {
        let uri = super::with_mandatory_query(uri, TransportType::Polling, Some(sid));

        let req = Request::builder()
            .method(http::Method::POST)
            .uri(uri)
            .body(BoxBody::new(Full::new(body.freeze())))
            .unwrap();

        let fut = svc.call(req);
        PostState::Pending {
            fut,
            bytes: BytesMut::new(),
        }
    }
}

#[derive(thiserror::Error)]
pub enum PollingError<S: PollingSvc> {
    #[error("http error: {0}")]
    Http(<S as PollingSvc>::Error),
    #[error("polling http body error: {0}")]
    HttpBody(<S as PollingSvc>::ResBodyError),
    #[error("packet error: {0}")]
    Packet(#[from] PacketParseError),
    #[error("server response error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("transport closed, it is not possible to send or receive data")]
    Closed,
}

impl<S: PollingSvc> fmt::Debug for PollingError<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PollingError::Http(err) => f.debug_tuple("Http").field(err).finish(),
            PollingError::HttpBody(err) => f.debug_tuple("HttpBody").field(err).finish(),
            PollingError::Packet(err) => f.debug_tuple("Packet").field(err).finish(),
            PollingError::Protocol(err) => f.debug_tuple("Protocol").field(err).finish(),
            PollingError::Closed => f.write_str("Closed"),
        }
    }
}

impl<S: PollingSvc> PollingError<S> {
    pub(crate) fn should_close(&self) -> bool {
        true
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("internal error: {status}")]
    ServerError { status: StatusCode },

    #[error("invalid request: {status}")]
    InvalidRequest { status: StatusCode },

    #[error("unknown transport")]
    UnknownTransport,
    #[error("unknown session id")]
    UnknownSessionID,
    #[error("bad handshake method")]
    BadHandshakeMethod,
    #[error("transport mismatch")]
    TransportMismatch,
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion,
}
impl ProtocolError {
    /// Tries to parse a response body and generate a [`ProtocolError`]
    /// from it.
    fn from_parts(parts: response::Parts, body: impl Buf) -> Self {
        #[derive(Deserialize)]
        struct ErrorBody {
            code: u8,
        }

        serde_json::from_reader(body.reader())
            .map(|ErrorBody { code }| Self::new(parts.status, Some(code)))
            .unwrap_or_else(|_| Self::new(parts.status, None))
    }

    fn new(status: StatusCode, code: Option<u8>) -> Self {
        match code {
            Some(0) => ProtocolError::UnknownTransport,
            Some(1) => ProtocolError::UnknownSessionID,
            Some(2) => ProtocolError::BadHandshakeMethod,
            Some(3) => ProtocolError::TransportMismatch,
            Some(5) => ProtocolError::UnsupportedProtocolVersion,
            _ if status.is_client_error() => ProtocolError::InvalidRequest { status },
            _ => ProtocolError::ServerError { status },
        }
    }
}

pin_project! {
    pub struct PollingTransport<S: PollingSvc>
    {
        pub(crate) svc: S,

        #[pin]
        poll_state: PollState<S::Future>,

        #[pin]
        post_state: PostState<S::Future>,

        close_state: ClosingState,

        base_uri: Uri,
        sid: Sid,
    }
}

impl<S: PollingSvc> PollingTransport<S> {
    pub async fn connect(
        svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Self, OpenPacket), PollingError<S>> {
        let req = super::build_connect_req(&config.uri, TransportType::Polling);
        tracing::trace!(?req, "handshake request");

        let res = svc.call(req).await.map_err(PollingError::Http)?;
        let body = res.collect().await.map_err(PollingError::HttpBody)?;

        let packet = Packet::parse(
            ProtocolVersion::V4,
            String::from_utf8(body.to_bytes().to_vec()).unwrap(),
        )?;

        match packet {
            Packet::Open(open) => {
                let poll_state = PollState::new_request(&svc, &config.uri, open.sid);
                let transport = PollingTransport {
                    svc,
                    poll_state,
                    post_state: PostState::default(),
                    close_state: ClosingState::default(),
                    sid: open.sid,
                    base_uri: config.uri.clone(),
                };

                tracing::debug!(?transport, ?open, "polling transport intialized");
                Ok((transport, open))
            }
            _ => Err(PollingError::Packet(PacketParseError::InvalidPacketType(
                None,
            ))),
        }
    }

    /// Tear the transport down: drop any in-flight request future (it must
    /// never be polled again once it completed), discard queued writes and
    /// refuse any further use.
    fn terminate(self: Pin<&mut Self>) {
        let mut proj = self.project();
        proj.poll_state.set(PollState::Closed);
        proj.post_state.set(PostState::Closed);
        *proj.close_state = ClosingState::Closed;
    }
}

impl<S: PollingSvc> Stream for PollingTransport<S> {
    type Item = Result<Packet, PollingError<S>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // the session is over (error or graceful close): the stream is fused.
        if self.close_state != ClosingState::Open {
            return Poll::Ready(None);
        }

        match ready!(self.as_mut().poll_next_inner(cx)) {
            Some(Err(err)) if err.should_close() => {
                self.terminate();
                Poll::Ready(Some(Err(err)))
            }
            res => Poll::Ready(res),
        }
    }
}

impl<S: PollingSvc> PollingTransport<S> {
    fn poll_next_inner(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Packet, PollingError<S>>>> {
        tracing::trace!(poll_state = ?self.poll_state, "polling");

        let mut proj = self.as_mut().project().poll_state.project();
        match proj {
            PollStateProj::Pending { ref mut fut } => {
                match ready!(fut.as_mut().poll(cx)) {
                    Ok(res) => {
                        let (parts, body) = res.into_parts();
                        let body = Box::pin(body);

                        if !parts.status.is_success() {
                            let body = body
                                .collect()
                                .now_or_never()
                                .unwrap()
                                .map_err(PollingError::HttpBody)?; //TODO: body collect state machine
                            let error = ProtocolError::from_parts(parts, body.aggregate());
                            return Poll::Ready(Some(Err(PollingError::Protocol(error))));
                        }

                        //TODO: implement limited body + Content-Type
                        let stream =
                            payload::decoder(body, None, ProtocolVersion::V4, 200).boxed_local();

                        self.project()
                            .poll_state
                            .set(PollState::Decoding { stream });

                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Err(err) => Poll::Ready(Some(Err(PollingError::Http(err)))),
                }
            }
            PollStateProj::Decoding { stream } => {
                if let Some(packet) = ready!(stream.poll_next(cx)) {
                    Poll::Ready(Some(packet.map_err(PollingError::from)))
                } else {
                    tracing::debug!(
                        sid = %self.sid,
                        "decoding stream ended, new polling req"
                    );
                    let request = PollState::new_request(&self.svc, &self.base_uri, self.sid);
                    self.project().poll_state.set(request);
                    //check if wake is needed
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            PollStateProj::Closed => Poll::Ready(None),
        }
    }

    fn poll_flush_inner(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), PollingError<S>>> {
        tracing::trace!(post_state = ?self.post_state, "flushing");
        let proj = self.as_mut().project().post_state.project();

        match proj {
            PostStateProj::Queuing { bytes } if bytes.is_empty() => Poll::Ready(Ok(())),
            PostStateProj::Queuing { bytes } => {
                let body = std::mem::take(bytes);
                //TODO: handle max body size from open packet
                let post_state = PostState::new_request(&self.svc, &self.base_uri, self.sid, body);
                self.project().post_state.set(post_state);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PostStateProj::Pending { fut, bytes } => {
                match ready!(fut.poll(cx)) {
                    Ok(res) => {
                        let (parts, res_body) = res.into_parts();
                        let res_body = res_body
                            .collect()
                            .now_or_never()
                            .unwrap()
                            .map_err(PollingError::HttpBody)?; //TODO error body collect
                        if !parts.status.is_success() {
                            let err = ProtocolError::from_parts(parts, res_body.aggregate());
                            return Poll::Ready(Err(PollingError::Protocol(err)));
                        }

                        let body = std::mem::take(bytes);
                        if body.is_empty() {
                            self.project().post_state.set(PostState::queuing(body));
                            Poll::Ready(Ok(())) // TODO: check response == ok
                        } else {
                            let post_state =
                                PostState::new_request(&self.svc, &self.base_uri, self.sid, body);
                            // resend another request immediately, the buffer was filled
                            // while the previous one was sent
                            self.project().post_state.set(post_state);
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    }
                    Err(err) => Poll::Ready(Err(PollingError::Http(err))),
                }
            }
            PostStateProj::Closed => Poll::Ready(Ok(())),
        }
    }
}

impl<S: PollingSvc> Sink<Packet> for PollingTransport<S> {
    type Error = PollingError<S>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.close_state != ClosingState::Open {
            Poll::Ready(Err(PollingError::Closed))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        tracing::trace!(post_state = ?self.post_state, "sending packet");
        if self.close_state != ClosingState::Open {
            return Err(PollingError::Closed);
        }
        self.project().post_state.encode(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match ready!(self.as_mut().poll_flush_inner(cx)) {
            Err(err) => {
                // any polling error is fatal: tear the transport down before
                // surfacing it so the completed request future can never be
                // polled again.
                self.terminate();
                Poll::Ready(Err(err))
            }
            ok => Poll::Ready(ok),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.close_state {
            ClosingState::Open => {
                // we dont need to call poll_ready on ourselve
                self.as_mut().start_send(Packet::Close)?;
                *self.project().close_state = ClosingState::Closing;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            ClosingState::Closing => {
                ready!(self.as_mut().poll_flush(cx))?;
                // the close packet is flushed: abort the held poll request
                // and refuse any further use.
                self.terminate();
                Poll::Ready(Ok(()))
            }
            ClosingState::Closed => Poll::Ready(Ok(())),
        }
    }
}

impl<F> PostState<F> {
    pub fn encode(self: Pin<&mut Self>, item: Packet) {
        const PACKET_SEPARATOR_V4: u8 = b'\x1e';
        let packet: Bytes = item.into();
        let bytes = match self.project() {
            PostStateProj::Queuing { bytes } => bytes,
            PostStateProj::Pending { bytes, .. } => bytes,
            // unreachable from `start_send` (gated on `close_state`), writes
            // on a closed transport are discarded.
            PostStateProj::Closed => return,
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
            Self::Pending { .. } => f.debug_struct("Pending").finish_non_exhaustive(),
            Self::Decoding { .. } => f.debug_struct("Decoding").finish_non_exhaustive(),
            Self::Closed => f.write_str("Closed"),
        }
    }
}
impl<F> fmt::Debug for PostState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queuing { bytes } => f.debug_struct("Queuing").field("bytes", bytes).finish(),
            Self::Pending { bytes, .. } => f
                .debug_struct("Pending")
                .field("bytes", bytes)
                .finish_non_exhaustive(),
            Self::Closed => f.write_str("Closed"),
        }
    }
}
impl<S: PollingSvc> fmt::Debug for PollingTransport<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollingTransport")
            .field("poll_state", &self.poll_state)
            .field("post_state", &self.post_state)
            .field("close_state", &self.close_state)
            .field("base_uri", &self.base_uri)
            .field("sid", &self.sid)
            .finish_non_exhaustive()
    }
}
