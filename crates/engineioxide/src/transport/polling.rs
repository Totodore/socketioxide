//! The polling transport module handles polling, post and init requests
use std::sync::Arc;

use bytes::Bytes;
use engineioxide_core::payload::{self, Payload};
use futures_util::{StreamExt, stream};
use http::{Request, Response, StatusCode, header::CONTENT_TYPE};
use http_body::Body;
use http_body_util::Full;
use tokio_util::future::FutureExt as _;

use engineioxide_core::{Packet, ProtocolVersion, Sid, TransportType};

use crate::{
    DisconnectReason, body::ResponseBody, engine::EngineIo, errors::Error,
    handler::EngineIoHandler, transport::make_open_packet,
};

/// Create a response for http request
fn http_response<B, D>(code: StatusCode, data: D, is_binary: bool) -> Response<ResponseBody<B>>
where
    D: Into<Bytes>,
{
    use http::header::*;
    let body: Bytes = data.into();
    let res = Response::builder()
        .status(code)
        .header(CONTENT_LENGTH, body.len());
    if is_binary {
        res.header(CONTENT_TYPE, "application/octet-stream")
    } else {
        res.header(CONTENT_TYPE, "text/plain; charset=UTF-8")
    }
    .body(ResponseBody::custom_response(Full::new(body)))
    .unwrap()
}

pub fn open_req<H, B, R>(
    engine: Arc<EngineIo<H>>,
    protocol: ProtocolVersion,
    req: Request<R>,
    supports_binary: bool,
) -> Result<Response<ResponseBody<B>>, Error>
where
    H: EngineIoHandler,
    B: Send + 'static,
{
    let socket = engine.create_session(
        protocol,
        TransportType::Polling,
        req.into_parts().0,
        supports_binary,
    );

    let packet = make_open_packet(TransportType::Polling, socket.id, &engine.config);

    socket.spawn_heartbeat(engine.config.ping_interval, engine.config.ping_timeout);

    let packet: String = Packet::Open(packet).into();
    let packet = {
        #[cfg(feature = "v3")]
        {
            let mut packet = packet;
            // The V3 protocol requires the packet length to be prepended to the packet.
            // It doesn't use a packet separator like the V4 protocol (and up).
            if protocol == ProtocolVersion::V3 {
                packet = format!("{}:{}", packet.chars().count(), packet);
            }
            packet
        }
        #[cfg(not(feature = "v3"))]
        packet
    };
    Ok(http_response(StatusCode::OK, packet, false))
}

/// Handle http polling request
///
/// If there is packet in the socket buffer, it will be sent immediately
/// Otherwise it will wait for the next packet to be sent from the socket
pub async fn polling_req<B, H>(
    engine: Arc<EngineIo<H>>,
    protocol: ProtocolVersion,
    sid: Sid,
) -> Result<Response<ResponseBody<B>>, Error>
where
    B: Send + 'static,
    H: EngineIoHandler,
{
    let socket = engine.get_socket(sid).ok_or(Error::UnknownSessionID(sid))?;
    if !socket.is_http() {
        return Err(Error::TransportMismatch);
    }

    if socket.is_upgrading() {
        #[cfg(feature = "tracing")]
        tracing::debug!(?sid, "socket is upgrading, sending NOOP packet");

        let data = payload::packet_encoder(Packet::Noop, socket.protocol, socket.supports_binary);

        let is_binary = false; // The noop packet is guaranteed to be serialized as text
        return Ok(http_response(StatusCode::OK, data, is_binary));
    }

    // If the socket is already locked, it means that the socket is being used by another request
    // In case of multiple http polling, session should be closed
    let mut rx = match socket.internal_rx.try_lock() {
        Ok(s) => s,
        Err(_) => {
            socket.close(DisconnectReason::MultipleHttpPollingError);
            return Err(Error::MultipleHttpPolling);
        }
    };

    #[cfg(feature = "tracing")]
    tracing::debug!(%sid, %protocol, supports_binary = socket.supports_binary, "polling request");

    let max_payload = engine.config.max_payload;

    // Prepend the packet peeked during the previous
    // polling request so it is encoded first, ahead of the newly received packets.
    let rx_stream = stream::iter(rx.peeked.take()).chain(rx_stream::EncoderStream::new(
        &mut rx,
        socket.volatile_rx.clone(), //TODO: do we loose any state from cloning volatile_rx?
    ));

    // Stop waiting for the next packet as soon as the session is closed. The combinator is biased
    // towards the encoder completion, so any buffered close packet is still flushed to the client.
    let payload = payload::encoder(rx_stream, protocol, socket.supports_binary, max_payload)
        .with_cancellation_token(&socket.cancellation_token)
        .await;

    let Some(Payload {
        data,
        has_binary,
        peeked,
    }) = payload
    else {
        #[cfg(feature = "tracing")]
        tracing::debug!(%sid, "session closed while polling, returning empty payload");
        return Ok(http_response(StatusCode::OK, "", false));
    };

    // set back the peeked packet so it can be read again
    // on the next polling request
    rx.peeked = peeked;

    #[cfg(feature = "tracing")]
    tracing::trace!(%sid, %protocol, supports_binary = socket.supports_binary, "sending data: {:?}", data);

    Ok(http_response(StatusCode::OK, data, has_binary))
}

/// Handle http polling post request
///
/// Split the body into packets and send them to the internal socket
pub async fn post_req<R, B, H>(
    engine: Arc<EngineIo<H>>,
    protocol: ProtocolVersion,
    sid: Sid,
    req: Request<R>,
) -> Result<Response<ResponseBody<B>>, Error>
where
    H: EngineIoHandler,
    R: Body + Send + Unpin + 'static,
    <R as Body>::Error: std::fmt::Debug,
    <R as Body>::Data: Send,
    B: Send + 'static,
{
    let socket = engine.get_socket(sid).ok_or(Error::UnknownSessionID(sid))?;
    if !socket.is_http() {
        return Err(Error::TransportMismatch);
    }

    let (parts, body) = req.into_parts();
    let content_type = parts.headers.get(CONTENT_TYPE);
    let packets = payload::decoder(body, content_type, protocol, engine.config.max_payload);
    futures_util::pin_mut!(packets);

    while let Some(packet) = packets.next().await {
        match packet {
            Ok(Packet::Close) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(%sid, "received close packet, closing session");
                socket.send(Packet::Noop).ok(); // if the send fails, let's forcefully close the socket
                engine.close_session(sid, DisconnectReason::TransportClose);
                break;
            }
            Ok(Packet::Pong | Packet::Ping) => socket
                .heartbeat_tx
                .try_send(())
                .map_err(|_| Error::HeartbeatTimeout),
            Ok(Packet::Message(msg)) => {
                engine.handler.on_message(msg, socket.clone());
                Ok(())
            }
            Ok(Packet::Binary(bin) | Packet::BinaryV3(bin)) => {
                engine.handler.on_binary(bin, socket.clone());
                Ok(())
            }
            Ok(p) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(%sid, "invalid packet received: {:?}", &p);
                Err(Error::BadPacket(p))
            }
            Err(e) => {
                #[cfg(feature = "tracing")]
                tracing::debug!(%sid, "could not parse packet: {e}");
                engine.close_session(sid, DisconnectReason::PacketParsingError);
                return Err(e.into());
            }
        }?;
    }
    Ok(http_response(StatusCode::OK, "ok", false))
}

mod rx_stream {
    use std::{
        pin::Pin,
        task::{Context, Poll, ready},
    };

    use futures_core::Stream;
    use pin_project_lite::pin_project;
    use tokio::sync::{mpsc, watch};
    use tokio_util::sync::ReusableBoxFuture;

    /// [`ReceiverStream`] is a stream that wraps a [`mpsc::Receiver`] by reference.
    /// Allowing to use it as a stream even if it is behind a mutex.
    pub struct ReceiverStream<'a, T> {
        inner: &'a mut mpsc::Receiver<T>,
    }

    impl<'a, T> ReceiverStream<'a, T> {
        pub fn new(inner: &'a mut mpsc::Receiver<T>) -> Self {
            Self { inner }
        }
    }

    impl<'a, T> Stream for ReceiverStream<'a, T> {
        type Item = T;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.inner.poll_recv(cx)
        }
    }

    pub struct WatchStream<T> {
        inner:
            ReusableBoxFuture<'static, (Result<(), watch::error::RecvError>, watch::Receiver<T>)>,
    }
    impl<T: Send + Sync + 'static> WatchStream<T> {
        pub fn new(rx: watch::Receiver<T>) -> Self {
            Self {
                inner: ReusableBoxFuture::new(make_future(rx)),
            }
        }
    }

    async fn make_future<T>(
        mut rx: watch::Receiver<T>,
    ) -> (Result<(), watch::error::RecvError>, watch::Receiver<T>) {
        let result = rx.changed().await;
        (result, rx)
    }

    impl<T: Clone + Send + Sync + 'static> Stream for WatchStream<T> {
        type Item = T;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let (result, mut rx) = ready!(self.inner.poll(cx));
            match result {
                Ok(_) => {
                    let received = (*rx.borrow_and_update()).clone();
                    self.inner.set(make_future(rx));
                    Poll::Ready(Some(received))
                }
                Err(_) => {
                    self.inner.set(make_future(rx));
                    Poll::Ready(None)
                }
            }
        }
    }

    impl<T> Unpin for WatchStream<T> {}

    pin_project! {
        /// An [`EncoderStream`] that wraps a [`WatchStream`] and a [`ReceiverStream`].
        /// It will poll the [`ReceiverStream`] first, and then the [`WatchStream`].
        ///
        /// This allow to have a priority on classic mpsc chan before polling volatile packets.
        pub struct EncoderStream<'a, T> {
            #[pin]
            watch: WatchStream<Option<T>>,
            #[pin]
            rx: ReceiverStream<'a, T>,
        }
    }

    impl<'a, T: Clone + Send + Sync + 'static> EncoderStream<'a, T> {
        pub fn new(rx: &'a mut mpsc::Receiver<T>, watch: watch::Receiver<Option<T>>) -> Self {
            Self {
                rx: ReceiverStream::new(rx),
                watch: WatchStream::new(watch),
            }
        }
    }

    impl<'a, T: Clone + Send + Sync + 'static> Stream for EncoderStream<'a, T> {
        type Item = T;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.project();
            match this.watch.poll_next(cx) {
                Poll::Ready(Some(v)) => Poll::Ready(v),
                Poll::Ready(None) | Poll::Pending => this.rx.poll_next(cx),
            }
        }
    }
}
