use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{Buf, Bytes};
use engineioxide_core::{
    OpenPacket, Packet, PacketParseError, ProtocolVersion, Sid, TransportType,
    payload::{self, BufList},
};
use futures_core::Stream;
use futures_util::{FutureExt, Sink, StreamExt};
use http::{Request, StatusCode, Uri, response};
use http_body_util::BodyExt;
use pin_project_lite::pin_project;
use serde::Deserialize;

use crate::{
    EngineIoClientConfig,
    flavors::{PollingBody, PollingSvc},
};

pin_project! {
    #[project = PollStateProj]
    enum PollState<F> {
        Pending {
            #[pin]
            fut: F
        },
        Decoding {
            //TODO: switch to concrete type
            #[pin]
            stream: Pin<Box<dyn Stream<Item = Result<Packet, PacketParseError>>>>
        },
        /// Polling is paused (upgrade in progress): the last poll completed
        /// and no new one is issued until [`PollingTransport::resume`].
        Paused,
        // Terminal state: the previous request future is dropped so it can
        // never be polled again after it completed with an error.
        Closed,
    }
}

pin_project! {
    #[project = PostStateProj]
    enum PostState<F> {
        /// No POST in flight: packets are appended to the payload until the next
        /// flush (or until the batch would exceed `max_payload`).
        Queuing {
            payload: BufList<Bytes>,
        },
        /// A POST is in flight. Packets sent meanwhile are appended to the
        /// `payload` and immeditely sent after the current one is done.
        ///
        /// A packet that would make `payload` exceed `max_payload` is held in
        /// `overflow` and opens the next batch. While it is set the sink
        /// applies backpressure: [`Sink::poll_ready`] waits for the in-flight
        /// request, so memory is bounded to one batch plus one packet.
        Pending {
            #[pin]
            fut: F,
            // Buf that is written to while the current request is in flight
            payload: BufList<Bytes>,
            // A potential overflowed packet that didn't fit in the payload
            overflow: Option<Bytes>,
        },
        // Terminal state: in-flight request and queued bytes are discarded.
        Closed,
    }
}

/// Length of the record separator between two packets of a v4 payload.
const PACKET_SEPARATOR_LEN: usize = 1;
const PACKET_SEPARATOR_V4: Bytes = Bytes::from_static(b"\x1e");

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
            payload: BufList::new(),
        }
    }
}
impl<F> PostState<F> {
    /// POST `payload` right away, streamed as queued, with the held back
    /// packet `rem` (if any) opening the next batch.
    fn new_request<S: PollingSvc<Future = F>>(
        svc: &mut S,
        uri: &Uri,
        sid: Sid,
        payload: BufList<Bytes>,
        rem: Option<Bytes>,
    ) -> Self {
        let uri = super::with_mandatory_query(uri, TransportType::Polling, Some(sid));
        let req = Request::builder()
            .method(http::Method::POST)
            .uri(uri)
            .body(PollingBody::new(payload))
            .unwrap();

        let fut = svc.call(req);

        // The remaining data is the new pending payload
        let mut payload = BufList::new();
        if let Some(rem) = rem {
            payload.push(rem);
        }
        PostState::Pending {
            fut,
            payload,
            overflow: None,
        }
    }

    /// `true` while a packet is held back behind a full queued batch: no
    /// more packets can be accepted until the in-flight request completes.
    fn is_saturated(&self) -> bool {
        matches!(
            self,
            PostState::Pending {
                overflow: Some(_),
                ..
            }
        )
    }
}

/// An empty batch always accepts the packet: a single packet over the
/// limit cannot be split, it is sent as is and rejected by the server.
fn is_batch_overflowed(batch: &impl Buf, packet_size: usize, max_payload: usize) -> bool {
    batch.has_remaining() && batch.remaining() + PACKET_SEPARATOR_LEN + packet_size > max_payload
}

impl<F> PollState<F> {
    fn new_request<S: PollingSvc<Future = F>>(svc: &mut S, base_uri: &Uri, sid: Sid) -> Self {
        let uri = super::with_mandatory_query(base_uri, TransportType::Polling, Some(sid));

        let req = Request::builder()
            .method(http::Method::GET)
            .uri(uri)
            .body(PollingBody::new_empty())
            .unwrap();

        let fut = svc.call(req);
        PollState::Pending { fut }
    }
}

/// An error that can occur during a polling transport.
#[derive(thiserror::Error)]
pub enum PollingError<S: PollingSvc> {
    /// The underlying polling service returned an error.
    #[error("http error: {0}")]
    Http(<S as PollingSvc>::Error),
    /// The polling HTTP body could not be parsed.
    #[error("polling http body error: {0}")]
    HttpBody(<S as PollingSvc>::ResBodyError),
    /// The packet could not be parsed.
    #[error("packet error: {0}")]
    Packet(#[from] PacketParseError),
    /// The server response could not be parsed.
    #[error("server response error: {0}")]
    Protocol(#[from] ProtocolError),
    /// The transport was closed, it is not possible to send or receive data.
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

/// A polling protocol error.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Server failed to process the request.
    #[error("internal error: {status}")]
    ServerError {
        /// The status code returned by the server.
        status: StatusCode,
    },

    /// Client has performed an invalid request.
    #[error("invalid request: {status}")]
    InvalidRequest {
        /// The status code returned by the server.
        status: StatusCode,
    },

    /// The transport is unknown.
    #[error("unknown transport")]
    UnknownTransport,

    /// The session ID is unknown.
    #[error("unknown session id")]
    UnknownSessionID,

    /// The handshake method is invalid.
    #[error("bad handshake method")]
    BadHandshakeMethod,

    /// The transport is unknown.
    #[error("transport mismatch")]
    TransportMismatch,

    /// The protocol version is unsupported.
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion,
}
impl ProtocolError {
    /// Tries to parse a response body and generate a [`ProtocolError`]
    /// from it.
    fn from_parts(parts: response::Parts, body: Option<impl Buf>) -> Self {
        let Some(body) = body else {
            return Self::new(parts.status, None);
        };

        #[derive(Deserialize)]
        struct ErrorBody {
            code: ErrorCode,
        }
        /// The reference server sends the code as a JSON number,
        /// engineioxide as a string.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ErrorCode {
            Number(u8),
            Text(String),
        }
        impl ErrorCode {
            fn get(&self) -> Option<u8> {
                match self {
                    ErrorCode::Number(code) => Some(*code),
                    ErrorCode::Text(code) => code.parse().ok(),
                }
            }
        }

        serde_json::from_reader(body.reader())
            .map(|ErrorBody { code }| Self::new(parts.status, code.get()))
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
    pub(crate) struct PollingTransport<S: PollingSvc>
    {
        pub(crate) svc: S,

        #[pin]
        poll_state: PollState<S::Future>,

        #[pin]
        post_state: PostState<S::Future>,

        close_state: ClosingState,
        // set while upgrading: the in-flight poll completes normally but
        // no new poll is issued afterwards.
        paused: bool,

        base_uri: Uri,
        max_payload: u64,
        sid: Sid,
    }
}

impl<S: PollingSvc> PollingTransport<S> {
    pub async fn connect(
        mut svc: S,
        config: &EngineIoClientConfig,
    ) -> Result<(Self, OpenPacket), PollingError<S>> {
        let req = super::build_connect_req(&config.uri, TransportType::Polling);
        tracing::trace!(?req, "handshake request");

        let res = svc.call(req).await.map_err(PollingError::Http)?;
        let (parts, body) = res.into_parts();
        let body = body.collect().await.map_err(PollingError::HttpBody)?;

        if !parts.status.is_success() {
            let error = ProtocolError::from_parts(parts, Some(body.aggregate()));
            return Err(PollingError::Protocol(error));
        }

        let body = String::from_utf8(body.to_bytes().to_vec())
            .map_err(|err| PacketParseError::InvalidUtf8Boundary(err.utf8_error()))?;
        let packet = Packet::parse(ProtocolVersion::V4, body)?;

        match packet {
            Packet::Open(open) => {
                let poll_state = PollState::new_request(&mut svc, &config.uri, open.sid);
                let transport = PollingTransport {
                    svc,
                    poll_state,
                    post_state: PostState::default(),
                    close_state: ClosingState::default(),
                    paused: false,
                    sid: open.sid,
                    max_payload: open.max_payload,
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
    pub(super) fn terminate(self: Pin<&mut Self>) {
        let mut proj = self.project();
        proj.poll_state.set(PollState::Closed);
        proj.post_state.set(PostState::Closed);
        *proj.close_state = ClosingState::Closed;
    }

    /// Pause polling (reference `pause()`, upgrade in progress): the
    /// in-flight poll completes normally, and no new poll is issued
    /// afterwards. Writes are unaffected.
    pub(super) fn pause(self: Pin<&mut Self>) {
        *self.project().paused = true;
    }

    /// `true` once paused with no poll in flight.
    pub(super) fn is_idle(&self) -> bool {
        matches!(self.poll_state, PollState::Paused)
    }

    /// Resume polling after a failed upgrade.
    pub(super) fn resume(&mut self) {
        self.paused = false;
        if matches!(self.poll_state, PollState::Paused) {
            self.poll_state = PollState::new_request(&mut self.svc, &self.base_uri, self.sid);
        }
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
            PollStateProj::Pending { ref mut fut } => match ready!(fut.as_mut().poll(cx)) {
                Ok(res) => {
                    let (parts, body) = res.into_parts();
                    let body = Box::pin(body);

                    if !parts.status.is_success() {
                        // best effort collect without state machine
                        let body = body
                            .collect()
                            .now_or_never()
                            .transpose()
                            .map_err(PollingError::HttpBody)?;
                        let error = ProtocolError::from_parts(parts, body.map(|b| b.aggregate()));
                        return Poll::Ready(Some(Err(PollingError::Protocol(error))));
                    }

                    let stream =
                        payload::decoder(body, None, ProtocolVersion::V4, self.max_payload)
                            .boxed_local();

                    self.project()
                        .poll_state
                        .set(PollState::Decoding { stream });

                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(err) => Poll::Ready(Some(Err(PollingError::Http(err)))),
            },
            PollStateProj::Decoding { stream } => {
                if let Some(packet) = ready!(stream.poll_next(cx)) {
                    Poll::Ready(Some(packet.map_err(PollingError::from)))
                } else {
                    let mut proj = self.project();
                    if *proj.paused {
                        tracing::debug!(sid = %proj.sid, "decoding stream ended, polling paused");
                        proj.poll_state.set(PollState::Paused);
                    } else {
                        tracing::debug!(sid = %proj.sid, "decoding stream ended, new polling req");
                        let request = PollState::new_request(proj.svc, proj.base_uri, *proj.sid);
                        proj.poll_state.set(request);
                    }
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            // nothing will be received until polling resumes
            PollStateProj::Paused => Poll::Pending,
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
            PostStateProj::Queuing { payload } if !payload.has_remaining() => Poll::Ready(Ok(())),
            PostStateProj::Queuing { payload } => {
                let body = std::mem::take(payload);
                let mut proj = self.project();
                let post_state =
                    PostState::new_request(proj.svc, proj.base_uri, *proj.sid, body, None);
                proj.post_state.set(post_state);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            PostStateProj::Pending {
                fut,
                payload,
                overflow,
            } => {
                match ready!(fut.poll(cx)) {
                    Ok(res) => {
                        let (parts, res_body) = res.into_parts();

                        if !parts.status.is_success() {
                            // best effort collect without state machine
                            let res_body = res_body
                                .collect()
                                .now_or_never()
                                .transpose()
                                .map_err(PollingError::HttpBody)?;
                            let error =
                                ProtocolError::from_parts(parts, res_body.map(|b| b.aggregate()));
                            return Poll::Ready(Err(PollingError::Protocol(error)));
                        }

                        let payload = std::mem::take(payload);
                        let overflow = overflow.take();
                        let mut proj = self.project();
                        if !payload.has_remaining() {
                            debug_assert!(overflow.is_none(), "overflow behind an empty batch");
                            proj.post_state.set(PostState::default());
                            Poll::Ready(Ok(()))
                        } else {
                            // the buffer was filled while the previous request was
                            // in flight: POST it right away. The packet held back
                            // behind it opens the next batch.
                            let post_state = PostState::new_request(
                                proj.svc,
                                proj.base_uri,
                                *proj.sid,
                                payload,
                                overflow,
                            );
                            proj.post_state.set(post_state);
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

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.close_state != ClosingState::Open {
            return Poll::Ready(Err(PollingError::Closed));
        }
        // backpressure: a packet is already held back behind a full queued
        // batch, wait for the in-flight request so the batch can be sent.
        while self.post_state.is_saturated() {
            ready!(self.as_mut().poll_flush(cx))?;
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Packet) -> Result<(), Self::Error> {
        tracing::trace!(post_state = ?self.post_state, "sending packet");
        if self.close_state != ClosingState::Open {
            return Err(PollingError::Closed);
        }
        // polling payloads are text: binary packets are base64 encoded
        let packet_size = item.get_size_hint(true);
        let max_payload = self.max_payload as usize;

        let mut proj = self.as_mut().project();
        match proj.post_state.as_mut().project() {
            PostStateProj::Queuing { payload }
                if is_batch_overflowed(payload, packet_size, max_payload) =>
            {
                tracing::debug!(
                    "pending buffer would exceed {max_payload} bytes, \
                        sending the current payload, current packet is deferred to the next batch"
                );
                let body = std::mem::take(payload);
                let post_state =
                    PostState::new_request(proj.svc, proj.base_uri, *proj.sid, body, None);
                proj.post_state.set(post_state);
                // the current packet opens the next batch
                proj.post_state.encode(item);
                Ok(())
            }
            PostStateProj::Pending {
                payload, overflow, ..
            } if is_batch_overflowed(payload, packet_size, max_payload) => {
                assert!(
                    overflow.is_none(),
                    "start_send called while the sink is not ready"
                );
                tracing::debug!(
                    "queued batch would exceed {max_payload} bytes, \
                        current packet is held back for the next batch"
                );
                *overflow = Some(item.into());
                Ok(())
            }
            PostStateProj::Queuing { .. } | PostStateProj::Pending { .. } => {
                proj.post_state.encode(item);
                Ok(())
            }

            PostStateProj::Closed => Err(PollingError::Closed),
        }
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
                ready!(self.as_mut().poll_ready(cx))?;
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
    /// Append `item` to the batch currently being filled.
    fn encode(self: Pin<&mut Self>, item: Packet) {
        let packet: Bytes = item.into();
        let Some(payload) = self.payload() else {
            return;
        };
        if payload.has_remaining() {
            payload.push(PACKET_SEPARATOR_V4);
        }
        payload.push(packet);
    }

    /// The batch new packets are appended to, `None` once closed.
    fn payload(self: Pin<&mut Self>) -> Option<&mut BufList<Bytes>> {
        match self.project() {
            PostStateProj::Queuing { payload } | PostStateProj::Pending { payload, .. } => {
                Some(payload)
            }
            PostStateProj::Closed => None,
        }
    }
}

impl<F> fmt::Debug for PollState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending { .. } => f.debug_struct("Pending").finish_non_exhaustive(),
            Self::Decoding { .. } => f.debug_struct("Decoding").finish_non_exhaustive(),
            Self::Paused => f.write_str("Paused"),
            Self::Closed => f.write_str("Closed"),
        }
    }
}
impl<F> fmt::Debug for PostState<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queuing { payload } => {
                f.debug_struct("Queuing").field("payload", payload).finish()
            }
            Self::Pending {
                payload, overflow, ..
            } => f
                .debug_struct("Pending")
                .field("payload", payload)
                .field("overflow", overflow)
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

#[cfg(test)]
mod tests {
    use std::pin::pin;

    use super::*;

    /// Packets are joined by the record separator: no leading separator, one
    /// between each pair, and the wire bytes come from the encoded packets.
    #[test]
    fn encode_joins_packets_with_the_record_separator() {
        let mut state = pin!(PostState::<std::future::Ready<()>>::default());
        for msg in ["a", "b", "c"] {
            state.as_mut().encode(Packet::Message(msg.into()));
        }
        let payload = state.as_mut().payload().unwrap();
        let bytes = payload.copy_to_bytes(payload.remaining());
        assert_eq!(bytes, "4a\x1e4b\x1e4c");
    }
}
