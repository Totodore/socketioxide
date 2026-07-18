//! Payload encoder and decoder for polling transport.

use crate::{Packet, PacketParseError, ProtocolVersion, packet::PacketBuf};

use bytes::Bytes;
use futures_util::Stream;
use http::HeaderValue;

mod buf;
mod decoder;
mod encoder;

const PACKET_SEPARATOR_V4: u8 = b'\x1e';
#[cfg(feature = "v3")]
const STRING_PACKET_SEPARATOR_V3: u8 = b':';
#[cfg(feature = "v3")]
const BINARY_PACKET_SEPARATOR_V3: u8 = 0xff;
#[cfg(feature = "v3")]
const STRING_PACKET_IDENTIFIER_V3: u8 = 0x00;
#[cfg(feature = "v3")]
const BINARY_PACKET_IDENTIFIER_V3: u8 = 0x01;
#[cfg(feature = "v3")]
const BINARY_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/octet-stream");

/// Decode a payload into a stream of packets.
pub fn decoder(
    body: impl http_body::Body<Error = impl std::fmt::Debug> + Unpin,
    #[allow(unused)] content_type: Option<&HeaderValue>,
    #[allow(unused)] protocol: ProtocolVersion,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, PacketParseError>> {
    #[cfg(feature = "tracing")]
    tracing::debug!(?content_type, %protocol, "decoding payload");

    #[cfg(feature = "v3")]
    {
        use futures_util::future::Either;

        let is_binary = content_type == Some(&BINARY_CONTENT_TYPE);
        match protocol {
            ProtocolVersion::V4 => Either::Left(decoder::v4_decoder(body, max_payload)),
            ProtocolVersion::V3 if is_binary => {
                Either::Right(Either::Left(decoder::v3_binary_decoder(body, max_payload)))
            }
            ProtocolVersion::V3 => {
                Either::Right(Either::Right(decoder::v3_string_decoder(body, max_payload)))
            }
        }
    }

    #[cfg(not(feature = "v3"))]
    {
        decoder::v4_decoder(body, max_payload)
    }
}

/// A payload to transmit to the client through http polling
pub struct Payload {
    /// The data of the payload.
    pub data: Bytes,
    /// Whether the payload contains binary data.
    pub has_binary: bool,
    /// A peeked packet buffer that could not be sent due to the max payload size.
    pub peeked: Option<PacketBuf>,
}
impl Payload {
    /// Creates a new payload with the given data and binary flag.
    pub fn new(data: Bytes, has_binary: bool) -> Self {
        Self {
            data,
            has_binary,
            peeked: None,
        }
    }
}

/// Encodes a payload into a byte stream.
pub async fn encoder(
    rx: impl Stream<Item = PacketBuf>,
    #[allow(unused)] protocol: ProtocolVersion,
    #[allow(unused)] supports_binary: bool,
    max_payload: u64,
) -> Payload {
    let mut rx = std::pin::pin!(peekable::Peekable::new(rx));

    #[cfg(feature = "v3")]
    let mut payload = match protocol {
        ProtocolVersion::V4 => encoder::v4_encoder(rx.as_mut(), max_payload).await,
        ProtocolVersion::V3 if supports_binary => {
            encoder::v3_binary_encoder(rx.as_mut(), max_payload).await
        }
        ProtocolVersion::V3 => encoder::v3_string_encoder(rx.as_mut(), max_payload).await,
    };

    #[cfg(not(feature = "v3"))]
    let mut payload = encoder::v4_encoder(rx.as_mut(), max_payload).await;

    // Recover any packet that was peeked but rejected (max payload exceeded) so
    // the caller can hand it back to the next encoding pass instead of losing it.
    payload.peeked = rx.take_peeked();

    payload
}

/// Encodes a single packet into a byte array.
pub fn packet_encoder(
    packet: Packet,
    #[allow(unused)] protocol: ProtocolVersion,
    #[allow(unused)] supports_binary: bool,
) -> Bytes {
    #[cfg(feature = "v3")]
    match protocol {
        ProtocolVersion::V4 => packet.into(),
        ProtocolVersion::V3 if supports_binary && packet.is_binary() => {
            use bytes::BytesMut;

            let size_hint = packet.get_size_hint(!supports_binary) + 2;
            let mut bytes = BytesMut::with_capacity(size_hint);
            encoder::v3_bin_packet_encoder(packet, &mut bytes);
            bytes.freeze()
        }
        ProtocolVersion::V3 => {
            use bytes::BytesMut;

            let size_hint = packet.get_size_hint(!supports_binary) + 2;
            let mut bytes = BytesMut::with_capacity(size_hint);
            encoder::v3_string_packet_encoder(packet, &mut bytes);
            bytes.freeze()
        }
    }

    #[cfg(not(feature = "v3"))]
    packet.into()
}

mod peekable {
    //! [`Peekable`] implementation taken from [`futures_util::stream::Peekable`] but
    //! with an [`Peekable::take_peeked`] method to get back a potentially peeked item.
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    use futures_util::{Stream, StreamExt as _, stream::Fuse};
    use pin_project_lite::pin_project;

    pin_project! {
        /// A `Stream` that implements a `peek` method.
        ///
        /// The `peek` method can be used to retrieve a reference
        /// to the next `Stream::Item` if available. A subsequent
        /// call to `poll` will return the owned item.
        #[derive(Debug)]
        #[must_use = "streams do nothing unless polled"]
        pub struct Peekable<St: Stream> {
            #[pin]
            stream: Fuse<St>,
            peeked: Option<St::Item>,
        }
    }

    impl<St: Stream> Peekable<St> {
        pub(super) fn new(stream: St) -> Self {
            Self {
                stream: stream.fuse(),
                peeked: None,
            }
        }

        /// Produces a future which retrieves a reference to the next item
        /// in the stream, or `None` if the underlying stream terminates.
        pub fn peek(self: Pin<&mut Self>) -> Peek<'_, St> {
            Peek { inner: Some(self) }
        }

        /// Takes the peeked item out of the buffer, if any.
        ///
        /// Only the `peeked` field is moved out, so this is safe to call
        /// through a pinned reference even when the underlying stream is `!Unpin`.
        pub fn take_peeked(self: Pin<&mut Self>) -> Option<St::Item> {
            self.project().peeked.take()
        }

        /// Peek retrieves a reference to the next item in the stream.
        ///
        /// This method polls the underlying stream and return either a reference
        /// to the next item if the stream is ready or passes through any errors.
        pub fn poll_peek(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<&St::Item>> {
            let mut this = self.project();

            Poll::Ready(loop {
                if this.peeked.is_some() {
                    break this.peeked.as_ref();
                } else if let Some(item) = futures_util::ready!(this.stream.as_mut().poll_next(cx))
                {
                    *this.peeked = Some(item);
                } else {
                    break None;
                }
            })
        }
    }

    impl<S: Stream> Stream for Peekable<S> {
        type Item = S::Item;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.project();
            if let Some(item) = this.peeked.take() {
                return Poll::Ready(Some(item));
            }
            this.stream.poll_next(cx)
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            let peek_len = usize::from(self.peeked.is_some());
            let (lower, upper) = self.stream.size_hint();
            let lower = lower.saturating_add(peek_len);
            let upper = match upper {
                Some(x) => x.checked_add(peek_len),
                None => None,
            };
            (lower, upper)
        }
    }

    pin_project! {
        /// Future for the [`Peekable::peek`](self::Peekable::peek) method.
        #[must_use = "futures do nothing unless polled"]
        pub struct Peek<'a, St: Stream> {
            inner: Option<Pin<&'a mut Peekable<St>>>,
        }
    }

    impl<'a, St> Future for Peek<'a, St>
    where
        St: Stream,
    {
        type Output = Option<&'a St::Item>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let inner = self.project().inner;
            if let Some(peekable) = inner {
                futures_util::ready!(peekable.as_mut().poll_peek(cx));

                inner.take().unwrap().poll_peek(cx)
            } else {
                panic!("Peek polled after completion")
            }
        }
    }
}
