//! Support of the `permessage-deflate` websocket extension ([RFC 7692]).
//!
//! `tungstenite` doesn't support websocket extensions: it can send a frame with the RSV1 bit set
//! through [`Message::Frame`], but it fails the connection as soon as it receives one. So each
//! direction is handled on its own:
//! - [`Deflater`] compresses the outgoing messages and sends them as raw frames.
//! - [`InflateStream`] sits below `tungstenite` and rewrites the incoming compressed frames as
//!   plain ones. Every other frame goes through untouched, so all the validation, size limits and
//!   protocol errors remain the job of `tungstenite`.
//!
//! `flate2` is used with its `zlib-rs` backend because the default one doesn't allow to set the
//! size of the LZ77 sliding window, which is required to honor `server_max_window_bits`.
//!
//! [RFC 7692]: https://www.rfc-editor.org/rfc/rfc7692

use std::{
    fmt::Write as _,
    io::{self, Cursor},
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{Buf, BufMut, BytesMut};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use http::{HeaderMap, HeaderValue, header::SEC_WEBSOCKET_EXTENSIONS};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::{
    Message,
    protocol::frame::{
        Frame, FrameHeader,
        coding::{Data as OpData, OpCode},
    },
};
use tokio_util::io::poll_read_buf;

use crate::config::{EngineIoConfig, PerMessageDeflateConfig};

use super::ws::MessageEncoder;

/// The name of the extension in the `Sec-WebSocket-Extensions` header.
const EXTENSION_NAME: &str = "permessage-deflate";

/// The largest size of the sliding window.
const MAX_WINDOW_BITS: u8 = 15;

/// The smallest size of the sliding window supported by `flate2`, whereas the RFC also allows 8.
const MIN_WINDOW_BITS: u8 = 9;

/// The end of a deflate block flushed with a sync flush. It is stripped from the compressed
/// messages on the wire and must be restored to decompress them. (RFC 7692 section 7.2.1 and 7.2.2)
const SYNC_FLUSH_TAIL: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

/// The maximum size of a rewritten frame.
/// A message is decompressed by chunks of this size to not allocate more than this per frame.
const MAX_INFLATED_CHUNK_SIZE: usize = 64 * 1024;

/// The result of a successful negotiation of the extension.
#[derive(Debug, PartialEq)]
pub(super) struct Negotiated {
    /// The value of the `Sec-WebSocket-Extensions` response header.
    pub response: HeaderValue,
    /// The compression context must not be kept between two outgoing messages.
    pub server_no_context_takeover: bool,
    /// The compression context is not kept between two incoming messages.
    pub client_no_context_takeover: bool,
    /// The size of the window used to compress the outgoing messages.
    pub server_max_window_bits: u8,
    /// The size of the window used to decompress the incoming messages.
    pub client_max_window_bits: u8,
}

/// Accepts the first offer of the `Sec-WebSocket-Extensions` request headers that can be honored.
///
/// This fn only depends on its arguments, so calling it when building the response and when
/// setting up the connection gives the same result.
pub(super) fn negotiate(
    config: &PerMessageDeflateConfig,
    headers: &HeaderMap,
) -> Option<Negotiated> {
    let offer = headers
        .get_all(SEC_WEBSOCKET_EXTENSIONS)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(parse_offer)
        // We are not able to compress with a window of 8 bits.
        .find(|offer| {
            offer
                .server_max_window_bits
                .is_none_or(|b| b >= MIN_WINDOW_BITS)
        })?;

    let server_no_context_takeover =
        config.server_no_context_takeover || offer.server_no_context_takeover;
    let client_no_context_takeover = config.client_no_context_takeover;
    let server_max_window_bits = config
        .server_max_window_bits
        .min(offer.server_max_window_bits.unwrap_or(MAX_WINDOW_BITS));

    let mut response = EXTENSION_NAME.to_owned();
    if server_no_context_takeover {
        response.push_str("; server_no_context_takeover");
    }
    if client_no_context_takeover {
        response.push_str("; client_no_context_takeover");
    }
    // The server must answer to this parameter if it was offered. Otherwise it is only needed
    // when using a smaller window than the default one. (RFC 7692 section 7.1.2.1)
    if offer.server_max_window_bits.is_some() || server_max_window_bits < MAX_WINDOW_BITS {
        write!(
            response,
            "; server_max_window_bits={server_max_window_bits}"
        )
        .unwrap();
    }
    // The server can only restrict the window of a client that supports it. (RFC 7692 section 7.1.2.2)
    let client_max_window_bits = match offer.client_max_window_bits {
        Some(hint) if config.client_max_window_bits < MAX_WINDOW_BITS => {
            let bits = config
                .client_max_window_bits
                .min(hint.unwrap_or(MAX_WINDOW_BITS));
            write!(response, "; client_max_window_bits={bits}").unwrap();
            bits
        }
        _ => MAX_WINDOW_BITS,
    };

    Some(Negotiated {
        response: response.parse().unwrap(), // we are only using constants
        server_no_context_takeover,
        client_no_context_takeover,
        server_max_window_bits,
        client_max_window_bits,
    })
}

/// The parameters of an extension offer that are relevant for the response.
///
/// There is no need to keep `client_no_context_takeover`: this is a hint,
/// and we are able to read any message whatever the client does with it.
struct Offer {
    server_no_context_takeover: bool,
    server_max_window_bits: Option<u8>,
    /// The parameter is present, with an optional value.
    client_max_window_bits: Option<Option<u8>>,
}

/// Parses an offer. Returns `None` if this is another extension or if it must be declined
/// because of an unknown, repeated or invalid parameter. (RFC 7692 section 7)
fn parse_offer(offer: &str) -> Option<Offer> {
    let mut params = offer.split(';').map(str::trim);
    if params.next()? != EXTENSION_NAME {
        return None;
    }

    let mut parsed = Offer {
        server_no_context_takeover: false,
        server_max_window_bits: None,
        client_max_window_bits: None,
    };
    let mut seen = 0u8;
    let mut first_seen = |bit: u8| {
        let first = seen & bit == 0;
        seen |= bit;
        first
    };
    for param in params {
        let (name, value) = match param.split_once('=') {
            Some((name, value)) => (name.trim(), Some(value.trim().trim_matches('"'))),
            None => (param, None),
        };
        match (name, value) {
            ("server_no_context_takeover", None) if first_seen(1) => {
                parsed.server_no_context_takeover = true;
            }
            ("client_no_context_takeover", None) if first_seen(2) => {}
            ("server_max_window_bits", Some(value)) if first_seen(4) => {
                parsed.server_max_window_bits = Some(parse_window_bits(value)?);
            }
            ("client_max_window_bits", None) if first_seen(8) => {
                parsed.client_max_window_bits = Some(None);
            }
            ("client_max_window_bits", Some(value)) if first_seen(8) => {
                parsed.client_max_window_bits = Some(Some(parse_window_bits(value)?));
            }
            _ => return None,
        }
    }
    Some(parsed)
}

/// A window size is a number between 8 and 15 without any leading zero. (RFC 7692 section 7.1.2)
fn parse_window_bits(value: &str) -> Option<u8> {
    if value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|bits| (8..=15).contains(bits))
}

/// Compresses the outgoing messages of a connection.
///
/// The compression context is allocated when compressing the first message
/// and it is only kept if the context takeover is enabled.
pub(super) struct Deflater {
    compress: Option<Compress>,
    level: Compression,
    threshold: usize,
    no_context_takeover: bool,
    window_bits: u8,
}

impl Deflater {
    pub fn new(config: &PerMessageDeflateConfig, negotiated: &Negotiated) -> Self {
        Self {
            compress: None,
            level: Compression::new(config.level),
            threshold: config.threshold,
            no_context_takeover: negotiated.server_no_context_takeover,
            window_bits: negotiated.server_max_window_bits,
        }
    }

    /// Compresses a message into a raw deflate stream without the trailing [`SYNC_FLUSH_TAIL`].
    fn deflate(&mut self, data: &[u8]) -> io::Result<Vec<u8>> {
        let (level, window_bits) = (self.level, self.window_bits);
        let compress = self
            .compress
            .get_or_insert_with(|| Compress::new_with_window_bits(level, false, window_bits));

        let start = compress.total_in();
        let mut out = Vec::with_capacity(data.len() / 2 + 64);
        loop {
            let consumed = (compress.total_in() - start) as usize;
            compress
                .compress_vec(&data[consumed..], &mut out, FlushCompress::Sync)
                .map_err(io::Error::other)?;
            // The flush is complete only if the output buffer wasn't filled.
            if compress.total_in() - start == data.len() as u64 && out.len() < out.capacity() {
                break;
            }
            out.reserve(out.capacity().max(64));
        }

        if !out.ends_with(&SYNC_FLUSH_TAIL) {
            return Err(io::Error::other("missing sync flush marker"));
        }
        out.truncate(out.len() - SYNC_FLUSH_TAIL.len());

        if self.no_context_takeover {
            self.compress = None;
        }
        Ok(out)
    }

    fn compressed_frame(&mut self, data: &[u8], opcode: OpData) -> io::Result<Message> {
        let payload = self.deflate(data)?;
        let mut frame = Frame::message(payload, OpCode::Data(opcode), true);
        frame.header_mut().rsv1 = true;
        Ok(Message::Frame(frame))
    }
}

impl MessageEncoder for Deflater {
    /// A message under the threshold is sent as is, which doesn't alter the compression context.
    ///
    /// A message that didn't shrink can't be sent as is once it is compressed:
    /// it is already part of the context, which would then differ from the one of the client.
    fn encode(&mut self, msg: Message) -> io::Result<Message> {
        match &msg {
            Message::Text(data) if data.len() >= self.threshold && !data.is_empty() => {
                self.compressed_frame(data.as_bytes(), OpData::Text)
            }
            Message::Binary(data) if data.len() >= self.threshold && !data.is_empty() => {
                self.compressed_frame(data, OpData::Binary)
            }
            _ => Ok(msg),
        }
    }
}

/// A compressed frame being rewritten as one or several plain frames.
struct InflatingFrame {
    /// The unmasked compressed payload.
    payload: BytesMut,
    /// The number of bytes given to the decompressor,
    /// this includes the [`SYNC_FLUSH_TAIL`] that follows the payload of the last frame of a message.
    pos: usize,
    /// The opcode of the next rewritten frame.
    opcode: OpCode,
    /// This is the last frame of the message.
    is_final: bool,
}

impl InflatingFrame {
    /// The size of the input of the decompressor for this frame.
    fn input_len(&self) -> usize {
        let tail = if self.is_final {
            SYNC_FLUSH_TAIL.len()
        } else {
            0
        };
        self.payload.len() + tail
    }

    /// The remaining input of the decompressor for this frame.
    fn input(&self) -> &[u8] {
        match self.payload.get(self.pos..) {
            Some(rest) if !rest.is_empty() || !self.is_final => rest,
            _ => &SYNC_FLUSH_TAIL[self.pos - self.payload.len()..],
        }
    }
}

/// A stream that decompresses the incoming messages of a connection for `tungstenite`.
///
/// Each compressed frame is rewritten as at least one plain frame so that `tungstenite`
/// always has the same view of the fragmentation as the client, and rejects what it must reject.
/// A rewritten frame is only produced once the previous one has been read. As `tungstenite` fails
/// when reaching the maximum message size, a decompression bomb can't use more memory than that.
///
/// The rewritten frames are masked with a zero key, so that `tungstenite` can still reject the
/// unmasked frames sent by the client while leaving the payload as is.
///
/// Writes go directly to the inner stream.
pub(super) struct InflateStream<S> {
    inner: S,
    /// The bytes read from the inner stream and not consumed yet.
    input: BytesMut,
    /// The rewritten bytes not read yet.
    output: BytesMut,
    /// The number of bytes from `input` to forward as is.
    passthrough: u64,
    /// The compressed frame being rewritten.
    frame: Option<InflatingFrame>,
    /// A compressed message is being received, and its last frame didn't arrive yet.
    in_compressed_message: bool,
    inflater: Option<Decompress>,
    no_context_takeover: bool,
    window_bits: u8,
    read_size: usize,
    max_frame_size: usize,
    chunk_size: usize,
}

impl<S> InflateStream<S> {
    pub fn new(inner: S, config: &EngineIoConfig, negotiated: &Negotiated) -> Self {
        Self {
            inner,
            input: BytesMut::new(),
            output: BytesMut::new(),
            passthrough: 0,
            frame: None,
            in_compressed_message: false,
            inflater: None,
            no_context_takeover: negotiated.client_no_context_takeover,
            // A client using a window of 8 bits is readable with a larger one.
            window_bits: negotiated.client_max_window_bits.max(MIN_WINDOW_BITS),
            read_size: config.ws_read_buffer_size.max(1),
            max_frame_size: config.ws_max_frame_size,
            chunk_size: MAX_INFLATED_CHUNK_SIZE.min(config.ws_max_frame_size).max(1),
        }
    }

    /// Only the first frame of a compressed message has the RSV1 bit set. (RFC 7692 section 6.1)
    ///
    /// Anything else is left to `tungstenite`, including the invalid frames.
    fn is_compressed(&self, header: &FrameHeader, len: u64) -> bool {
        let compressed = match header.opcode {
            OpCode::Data(OpData::Text | OpData::Binary) => {
                header.rsv1 && !self.in_compressed_message
            }
            OpCode::Data(OpData::Continue) => !header.rsv1 && self.in_compressed_message,
            _ => false,
        };
        compressed
            && !header.rsv2
            && !header.rsv3
            && header.mask.is_some()
            && len <= self.max_frame_size as u64
    }

    /// Looks at the frame at the start of the input. Returns `false` if more data is needed.
    fn next_frame(&mut self) -> bool {
        let mut cursor = Cursor::new(&self.input[..]);
        let (header, len) = match FrameHeader::parse(&mut cursor) {
            Ok(Some(frame)) => frame,
            Ok(None) => return false,
            Err(_) => {
                // tungstenite fails on the same error, so whatever follows doesn't matter.
                self.passthrough = u64::MAX;
                return true;
            }
        };
        let header_len = cursor.position() as usize;

        if !self.is_compressed(&header, len) {
            self.passthrough = (header_len as u64).saturating_add(len);
            return true;
        }

        // The length can't overflow as it is lower than the maximum frame size.
        let len = len as usize;
        if self.input.len() < header_len + len {
            return false;
        }
        self.input.advance(header_len);
        let mut payload = self.input.split_to(len);
        if let Some(mask) = header.mask {
            for (byte, key) in payload.iter_mut().zip(mask.iter().cycle()) {
                *byte ^= key;
            }
        }

        self.in_compressed_message = !header.is_final;
        self.frame = Some(InflatingFrame {
            payload,
            pos: 0,
            opcode: header.opcode,
            is_final: header.is_final,
        });
        true
    }

    /// Decompresses a chunk of the current frame and writes it to the output as a plain frame.
    fn inflate_chunk(&mut self, mut frame: InflatingFrame) -> io::Result<()> {
        let window_bits = self.window_bits;
        let inflater = self
            .inflater
            .get_or_insert_with(|| Decompress::new_with_window_bits(false, window_bits));

        let mut chunk = Vec::with_capacity(self.chunk_size);
        while chunk.len() < self.chunk_size {
            let (total_in, total_out) = (inflater.total_in(), inflater.total_out());
            let status = inflater
                .decompress_vec(frame.input(), &mut chunk, FlushDecompress::None)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let consumed = (inflater.total_in() - total_in) as usize;
            let progress = consumed > 0 || inflater.total_out() != total_out;
            frame.pos += consumed;

            // A client may flush a message with a final deflate block. (RFC 7692 section 7.2.3.4)
            // What follows starts a new stream, so this loses the window. This doesn't matter in
            // practice as a client doing this has no context to refer to in its next messages.
            if status == Status::StreamEnd {
                inflater.reset(false);
                // The RFC requires a padding byte that makes the tail a valid empty block.
                // Without it, the tail would be an incomplete block corrupting the next message.
                if frame.pos >= frame.payload.len() {
                    frame.pos = frame.input_len();
                }
            }
            if !progress {
                if frame.pos != frame.input_len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unable to decompress the message",
                    ));
                }
                break;
            }
        }

        // If the chunk is full there may be more data coming out of the decompressor,
        // and we would then have to produce an additional and possibly empty frame.
        let done = frame.pos == frame.input_len() && chunk.len() < self.chunk_size;
        let header = FrameHeader {
            is_final: frame.is_final && done,
            opcode: frame.opcode,
            mask: Some([0; 4]),
            ..Default::default()
        };
        self.output
            .reserve(header.len(chunk.len() as u64) + chunk.len());
        header
            .format(chunk.len() as u64, &mut (&mut self.output).writer())
            .map_err(io::Error::other)?;
        self.output.extend_from_slice(&chunk);

        if !done {
            frame.opcode = OpCode::Data(OpData::Continue);
            self.frame = Some(frame);
        } else if frame.is_final && self.no_context_takeover {
            self.inflater = None;
        }
        Ok(())
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for InflateStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        loop {
            if !this.output.is_empty() {
                let len = this.output.len().min(buf.remaining());
                buf.put_slice(&this.output[..len]);
                this.output.advance(len);
                if this.output.is_empty() && this.frame.is_none() {
                    // Don't hold a whole chunk for an idle connection.
                    this.output = BytesMut::new();
                }
                return Poll::Ready(Ok(()));
            }

            if let Some(frame) = this.frame.take() {
                this.inflate_chunk(frame)?;
                continue;
            }

            if this.passthrough > 0 {
                if !this.input.is_empty() {
                    let len = (this.input.len() as u64)
                        .min(buf.remaining() as u64)
                        .min(this.passthrough);
                    buf.put_slice(&this.input[..len as usize]);
                    this.input.advance(len as usize);
                    this.passthrough -= len;
                    return Poll::Ready(Ok(()));
                }
            } else if this.next_frame() {
                continue;
            }

            this.input.reserve(this.read_size);
            let read = ready!(poll_read_buf(
                Pin::new(&mut this.inner),
                cx,
                &mut this.input
            ))?;
            if read == 0 {
                if this.input.is_empty() {
                    return Poll::Ready(Ok(()));
                }
                // Let tungstenite handle what remains of the truncated frame before the EOF.
                this.passthrough = u64::MAX;
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for InflateStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use tokio::io::{AsyncWriteExt, DuplexStream, duplex};
    use tokio_tungstenite::{
        WebSocketStream,
        tungstenite::{
            self,
            error::{CapacityError, ProtocolError},
            protocol::{Role, WebSocketConfig, frame::coding::Control},
        },
    };

    use super::*;

    const MASK: [u8; 4] = [0x12, 0x34, 0x56, 0x78];

    fn offer(value: &str) -> Option<Negotiated> {
        offer_with(&PerMessageDeflateConfig::default(), value)
    }

    fn offer_with(config: &PerMessageDeflateConfig, value: &str) -> Option<Negotiated> {
        let mut headers = HeaderMap::new();
        headers.insert(SEC_WEBSOCKET_EXTENSIONS, value.parse().unwrap());
        negotiate(config, &headers)
    }

    fn response(value: &str) -> Option<HeaderValue> {
        offer(value).map(|n| n.response)
    }

    #[test]
    fn negotiate_accepts_valid_offers() {
        let plain = Some(HeaderValue::from_static("permessage-deflate"));
        assert_eq!(response("permessage-deflate"), plain);
        // The chrome and firefox offers.
        assert_eq!(
            response("permessage-deflate; client_max_window_bits"),
            plain
        );
        assert_eq!(
            response("permessage-deflate;client_max_window_bits=10"),
            plain
        );
        assert_eq!(
            response("permessage-deflate; client_max_window_bits=\"15\""),
            plain
        );
        // The client hints that it won't use the context, there is nothing to do on our side.
        assert_eq!(
            response("permessage-deflate; client_no_context_takeover"),
            plain
        );
        assert_eq!(response("foo, permessage-deflate"), plain);
    }

    #[test]
    fn negotiate_answers_server_parameters() {
        let negotiated = offer("permessage-deflate; server_no_context_takeover").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; server_no_context_takeover"
        );
        assert!(negotiated.server_no_context_takeover);
        assert!(!negotiated.client_no_context_takeover);

        assert_eq!(
            response("permessage-deflate; server_max_window_bits=15").unwrap(),
            "permessage-deflate; server_max_window_bits=15"
        );
    }

    #[test]
    fn negotiate_applies_config() {
        let config = PerMessageDeflateConfig::default()
            .server_no_context_takeover(true)
            .client_no_context_takeover(true);
        let negotiated = offer_with(&config, "permessage-deflate").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; server_no_context_takeover; client_no_context_takeover"
        );
        assert!(negotiated.server_no_context_takeover);
        assert!(negotiated.client_no_context_takeover);
    }

    #[test]
    fn negotiate_declines_invalid_offers() {
        for value in [
            "",
            "foo",
            "permessage-deflate-foo",
            "permessage-deflate; foo",
            "permessage-deflate; foo=1",
            "permessage-deflate; server_no_context_takeover=1",
            "permessage-deflate; client_no_context_takeover=1",
            "permessage-deflate; server_no_context_takeover; server_no_context_takeover",
            "permessage-deflate; client_max_window_bits; client_max_window_bits=10",
            "permessage-deflate; server_max_window_bits",
            "permessage-deflate; server_max_window_bits=16",
            "permessage-deflate; server_max_window_bits=015",
            "permessage-deflate; server_max_window_bits=+15",
            "permessage-deflate; client_max_window_bits=7",
            "permessage-deflate; client_max_window_bits=foo",
            "permessage-deflate; client_max_window_bits=",
        ] {
            assert_eq!(response(value), None, "{value}");
        }
    }

    #[test]
    fn negotiate_declines_window_of_8_bits() {
        assert_eq!(
            response("permessage-deflate; server_max_window_bits=8"),
            None
        );
        // The client can give a fallback offer.
        assert_eq!(
            response("permessage-deflate; server_max_window_bits=8, permessage-deflate").unwrap(),
            "permessage-deflate"
        );
    }

    #[test]
    fn negotiate_server_window() {
        let negotiated = offer("permessage-deflate; server_max_window_bits=10").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; server_max_window_bits=10"
        );
        assert_eq!(negotiated.server_max_window_bits, 10);

        // The configuration applies even if the client didn't ask for it, and the lowest wins.
        let config = PerMessageDeflateConfig::default().server_max_window_bits(12);
        let negotiated = offer_with(&config, "permessage-deflate").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; server_max_window_bits=12"
        );
        assert_eq!(negotiated.server_max_window_bits, 12);
        let negotiated =
            offer_with(&config, "permessage-deflate; server_max_window_bits=10").unwrap();
        assert_eq!(negotiated.server_max_window_bits, 10);
        let negotiated =
            offer_with(&config, "permessage-deflate; server_max_window_bits=14").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; server_max_window_bits=12"
        );
    }

    #[test]
    fn negotiate_client_window() {
        let config = PerMessageDeflateConfig::default().client_max_window_bits(12);

        // A client that doesn't support it can't be restricted
        let negotiated = offer_with(&config, "permessage-deflate").unwrap();
        assert_eq!(negotiated.response, "permessage-deflate");
        assert_eq!(negotiated.client_max_window_bits, 15);

        let negotiated = offer_with(&config, "permessage-deflate; client_max_window_bits").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; client_max_window_bits=12"
        );
        assert_eq!(negotiated.client_max_window_bits, 12);

        let negotiated =
            offer_with(&config, "permessage-deflate; client_max_window_bits=10").unwrap();
        assert_eq!(
            negotiated.response,
            "permessage-deflate; client_max_window_bits=10"
        );
        assert_eq!(negotiated.client_max_window_bits, 10);

        // Nothing to answer without any restriction in the configuration
        let negotiated = offer("permessage-deflate; client_max_window_bits=10").unwrap();
        assert_eq!(negotiated.response, "permessage-deflate");
        assert_eq!(negotiated.client_max_window_bits, 15);
    }

    #[test]
    fn negotiate_reads_all_headers() {
        let mut headers = HeaderMap::new();
        headers.append(SEC_WEBSOCKET_EXTENSIONS, "foo".parse().unwrap());
        headers.append(
            SEC_WEBSOCKET_EXTENSIONS,
            "permessage-deflate".parse().unwrap(),
        );
        assert!(negotiate(&PerMessageDeflateConfig::default(), &headers).is_some());
        assert!(negotiate(&PerMessageDeflateConfig::default(), &HeaderMap::new()).is_none());
    }

    fn negotiated(
        server_no_context_takeover: bool,
        client_no_context_takeover: bool,
    ) -> Negotiated {
        Negotiated {
            response: HeaderValue::from_static(EXTENSION_NAME),
            server_no_context_takeover,
            client_no_context_takeover,
            server_max_window_bits: MAX_WINDOW_BITS,
            client_max_window_bits: MAX_WINDOW_BITS,
        }
    }

    fn deflater(threshold: usize, no_context_takeover: bool) -> Deflater {
        Deflater::new(
            &PerMessageDeflateConfig::default().threshold(threshold),
            &negotiated(no_context_takeover, false),
        )
    }

    /// Decompresses a message like a client would, with its context.
    fn inflate(inflater: &mut Decompress, payload: &[u8]) -> Vec<u8> {
        let input = [payload, &SYNC_FLUSH_TAIL].concat();
        let mut out = Vec::with_capacity(payload.len() * 16 + 1024);
        let start = inflater.total_in() as usize;
        while (inflater.total_in() as usize) - start < input.len() {
            let consumed = inflater.total_in() as usize - start;
            out.reserve(1024);
            inflater
                .decompress_vec(&input[consumed..], &mut out, FlushDecompress::None)
                .unwrap();
        }
        out
    }

    /// Compresses a message like a client would, with its context.
    fn deflate(compress: &mut Compress, data: &[u8]) -> Vec<u8> {
        let mut deflater = Deflater {
            compress: Some(Compress::new(Compression::default(), false)),
            level: Compression::default(),
            threshold: 0,
            no_context_takeover: false,
            window_bits: MAX_WINDOW_BITS,
        };
        std::mem::swap(deflater.compress.as_mut().unwrap(), compress);
        let out = deflater.deflate(data).unwrap();
        std::mem::swap(deflater.compress.as_mut().unwrap(), compress);
        out
    }

    fn unwrap_frame(msg: Message) -> Frame {
        match msg {
            Message::Frame(frame) => frame,
            msg => panic!("expected a frame, got {msg:?}"),
        }
    }

    #[test]
    fn encode_leaves_small_messages_as_is() {
        let mut deflater = deflater(10, false);
        let text = Message::Text("too small".into());
        assert_eq!(deflater.encode(text.clone()).unwrap(), text);
        let binary = Message::Binary(vec![0; 9].into());
        assert_eq!(deflater.encode(binary.clone()).unwrap(), binary);
        assert!(
            deflater.compress.is_none(),
            "context should be lazily created"
        );
    }

    #[test]
    fn encode_leaves_empty_messages_and_control_frames_as_is() {
        let mut deflater = deflater(0, false);
        for msg in [
            Message::Text("".into()),
            Message::Binary(Vec::new().into()),
            Message::Ping(vec![0; 10].into()),
            Message::Close(None),
        ] {
            assert_eq!(deflater.encode(msg.clone()).unwrap(), msg);
        }
    }

    #[test]
    fn encode_sets_frame_header() {
        let mut deflater = deflater(0, false);
        let data = "a".repeat(1024);

        let frame = unwrap_frame(deflater.encode(Message::Text(data.clone().into())).unwrap());
        let header = frame.header();
        assert!(header.rsv1 && header.is_final && !header.rsv2 && !header.rsv3);
        assert_eq!(header.opcode, OpCode::Data(OpData::Text));
        assert!(frame.payload().len() < data.len());
        assert!(!frame.payload().ends_with(&SYNC_FLUSH_TAIL));

        let frame = unwrap_frame(deflater.encode(Message::Binary(data.into())).unwrap());
        assert_eq!(frame.header().opcode, OpCode::Data(OpData::Binary));
    }

    #[test]
    fn encode_with_context_takeover() {
        let mut deflater = deflater(0, false);
        let mut client = Decompress::new(false);
        let data = "hello world, this is a test ".repeat(10);

        let msg = || Message::Text(data.clone().into());
        let first = unwrap_frame(deflater.encode(msg()).unwrap());
        let second = unwrap_frame(deflater.encode(msg()).unwrap());
        assert!(second.len() < first.len(), "the context should be reused");

        assert_eq!(inflate(&mut client, first.payload()), data.as_bytes());
        assert_eq!(inflate(&mut client, second.payload()), data.as_bytes());
        assert!(deflater.compress.is_some());
    }

    #[test]
    fn encode_without_context_takeover() {
        let mut deflater = deflater(0, true);
        let data = "hello world, this is a test ".repeat(10);

        for _ in 0..2 {
            let msg = Message::Text(data.clone().into());
            let frame = unwrap_frame(deflater.encode(msg).unwrap());
            // Each message must be readable on its own
            let mut client = Decompress::new(false);
            assert_eq!(inflate(&mut client, frame.payload()), data.as_bytes());
            assert!(deflater.compress.is_none(), "context should be released");
        }
    }

    /// A pseudo random and not compressible payload.
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    #[test]
    fn encode_handles_payloads_that_do_not_shrink() {
        // A message that doesn't compress exceeds the first output buffer.
        for len in [1, 2, 63, 64, 1024, 4096, 1 << 20] {
            let data = noise(len);
            let frame = unwrap_frame(
                deflater(0, false)
                    .encode(Message::Binary(data.clone().into()))
                    .unwrap(),
            );
            let mut client = Decompress::new(false);
            assert_eq!(inflate(&mut client, frame.payload()), data, "len {len}");
        }
    }

    /// Build the bytes of a client frame.
    fn client_frame(
        opcode: OpCode,
        is_final: bool,
        rsv1: bool,
        mask: Option<[u8; 4]>,
        payload: &[u8],
    ) -> Vec<u8> {
        let header = FrameHeader {
            is_final,
            rsv1,
            opcode,
            mask,
            ..Default::default()
        };
        let mut out = Vec::new();
        header.format(payload.len() as u64, &mut out).unwrap();
        match mask {
            Some(mask) => out.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, k)| b ^ k)),
            None => out.extend_from_slice(payload),
        }
        out
    }

    fn compressed(opcode: OpData, payload: &[u8]) -> Vec<u8> {
        client_frame(OpCode::Data(opcode), true, true, Some(MASK), payload)
    }

    fn compress_once(data: &[u8]) -> Vec<u8> {
        deflate(&mut Compress::new(Compression::default(), false), data)
    }

    fn server_config() -> EngineIoConfig {
        EngineIoConfig::default()
    }

    /// Create a server websocket reading through an [`InflateStream`], and the socket of its client.
    async fn server(
        config: &EngineIoConfig,
        client_no_context_takeover: bool,
    ) -> (WebSocketStream<InflateStream<DuplexStream>>, DuplexStream) {
        let (client, server) = duplex(1024);
        let stream = InflateStream::new(
            server,
            config,
            &negotiated(false, client_no_context_takeover),
        );
        let ws_config = WebSocketConfig::default()
            .read_buffer_size(config.ws_read_buffer_size)
            .max_message_size(Some(config.ws_max_message_size))
            .max_frame_size(Some(config.ws_max_frame_size));
        let ws = WebSocketStream::from_raw_socket(stream, Role::Server, Some(ws_config)).await;
        (ws, client)
    }

    /// Writes bytes from the client. It is done in a task because
    /// the size of the duplex is lower than some of the messages.
    fn send(mut client: DuplexStream, bytes: Vec<u8>) -> tokio::task::JoinHandle<DuplexStream> {
        tokio::spawn(async move {
            client.write_all(&bytes).await.ok();
            client
        })
    }

    #[tokio::test]
    async fn inflate_text_and_binary() {
        let (mut ws, client) = server(&server_config(), false).await;
        let mut bytes = compressed(OpData::Text, &compress_once(b"hello"));
        bytes.extend(compressed(OpData::Binary, &compress_once(b"world")));
        let _client = send(client, bytes);

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("hello".into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(b"world".to_vec().into())
        );
    }

    #[tokio::test]
    async fn inflate_empty_message() {
        let (mut ws, client) = server(&server_config(), false).await;
        // RFC 7692 section 7.2.3.6
        let _client = send(client, compressed(OpData::Binary, &[0x02, 0x00]));
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(Vec::new().into())
        );
    }

    #[tokio::test]
    async fn plain_frames_go_through() {
        let (mut ws, client) = server(&server_config(), false).await;
        let mut bytes = client_frame(
            OpCode::Data(OpData::Text),
            true,
            false,
            Some(MASK),
            b"plain",
        );
        bytes.extend(compressed(OpData::Text, &compress_once(b"compressed")));
        bytes.extend(client_frame(
            OpCode::Control(Control::Ping),
            true,
            false,
            Some(MASK),
            b"ping",
        ));
        bytes.extend(client_frame(
            OpCode::Data(OpData::Binary),
            true,
            false,
            Some(MASK),
            b"plain",
        ));
        let _client = send(client, bytes);

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("plain".into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("compressed".into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Ping(b"ping".to_vec().into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(b"plain".to_vec().into())
        );
    }

    #[tokio::test]
    async fn inflate_with_context_takeover() {
        let (mut ws, client) = server(&server_config(), false).await;
        let data = "hello world, this is a test ".repeat(10);
        let mut compress = Compress::new(Compression::default(), false);
        let first = deflate(&mut compress, data.as_bytes());
        let second = deflate(&mut compress, data.as_bytes());
        assert!(second.len() < first.len());

        let mut bytes = compressed(OpData::Text, &first);
        bytes.extend(compressed(OpData::Text, &second));
        let _client = send(client, bytes);

        for _ in 0..2 {
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(data.clone().into())
            );
        }
        assert!(ws.get_ref().inflater.is_some());
    }

    #[tokio::test]
    async fn inflate_without_context_takeover() {
        let (mut ws, client) = server(&server_config(), true).await;
        let mut bytes = compressed(OpData::Text, &compress_once(b"hello"));
        bytes.extend(compressed(OpData::Text, &compress_once(b"hello")));
        let _client = send(client, bytes);

        for _ in 0..2 {
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("hello".into())
            );
            assert!(
                ws.get_ref().inflater.is_none(),
                "context should be released"
            );
        }
    }

    #[tokio::test]
    async fn inflate_fragmented_message_with_interleaved_control_frame() {
        let (mut ws, client) = server(&server_config(), false).await;
        let data = "hello world, this is a test ".repeat(100);
        let payload = compress_once(data.as_bytes());
        let (start, end) = payload.split_at(payload.len() / 2);
        let (middle, end) = end.split_at(end.len() / 2);

        let mut bytes = client_frame(OpCode::Data(OpData::Text), false, true, Some(MASK), start);
        bytes.extend(client_frame(
            OpCode::Control(Control::Ping),
            true,
            false,
            Some(MASK),
            b"ping",
        ));
        bytes.extend(client_frame(
            OpCode::Data(OpData::Continue),
            false,
            false,
            Some(MASK),
            middle,
        ));
        bytes.extend(client_frame(
            OpCode::Data(OpData::Continue),
            true,
            false,
            Some(MASK),
            end,
        ));
        // The stream goes back to its initial state after a fragmented message.
        bytes.extend(client_frame(
            OpCode::Data(OpData::Text),
            true,
            false,
            Some(MASK),
            b"plain",
        ));
        let _client = send(client, bytes);

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Ping(b"ping".to_vec().into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text(data.into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("plain".into())
        );
    }

    #[tokio::test]
    async fn inflate_with_empty_fragment() {
        let (mut ws, client) = server(&server_config(), false).await;
        let payload = compress_once(b"hello");
        let mut bytes = client_frame(OpCode::Data(OpData::Text), false, true, Some(MASK), &[]);
        bytes.extend(client_frame(
            OpCode::Data(OpData::Continue),
            true,
            false,
            Some(MASK),
            &payload,
        ));
        let _client = send(client, bytes);
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("hello".into())
        );
    }

    #[tokio::test]
    async fn inflate_bytes_received_one_by_one() {
        let (mut ws, mut client) = server(&server_config(), false).await;
        let data = "hello world, this is a test ".repeat(100);
        let mut bytes = compressed(OpData::Text, &compress_once(data.as_bytes()));
        bytes.extend(client_frame(
            OpCode::Data(OpData::Text),
            true,
            false,
            Some(MASK),
            b"plain",
        ));

        tokio::spawn(async move {
            for byte in bytes {
                client.write_all(&[byte]).await.unwrap();
                client.flush().await.unwrap();
                tokio::task::yield_now().await;
            }
            client
        });

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text(data.into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("plain".into())
        );
    }

    #[tokio::test]
    async fn inflate_message_larger_than_max_frame_size() {
        let mut config = server_config();
        config.ws_max_frame_size = 16 * 1024;
        let (mut ws, client) = server(&config, false).await;

        // This is compressed under the maximum frame size, but inflates over it.
        let data = "hello world, this is a test ".repeat(40_000);
        let payload = compress_once(data.as_bytes());
        assert!(payload.len() < config.ws_max_frame_size);
        let _client = send(client, compressed(OpData::Text, &payload));

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text(data.into())
        );
    }

    #[tokio::test]
    async fn inflate_exact_chunk_sizes() {
        // These sizes end exactly on a chunk, which results in an additional empty fragment.
        for len in [
            MAX_INFLATED_CHUNK_SIZE - 1,
            MAX_INFLATED_CHUNK_SIZE,
            MAX_INFLATED_CHUNK_SIZE + 1,
            MAX_INFLATED_CHUNK_SIZE * 2,
        ] {
            let (mut ws, client) = server(&server_config(), false).await;
            let data = noise(len);
            let _client = send(client, compressed(OpData::Binary, &compress_once(&data)));
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(data.into()),
                "len {len}"
            );
        }
    }

    #[tokio::test]
    async fn inflate_empty_stored_block_first() {
        // RFC 7692 section 7.2.3.6, the other way of sending an empty message.
        let (mut ws, client) = server(&server_config(), false).await;
        let mut bytes = compressed(OpData::Binary, &[0x00]);
        bytes.extend(compressed(OpData::Text, &compress_once(b"hello")));
        let _client = send(client, bytes);

        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Binary(Vec::new().into())
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text("hello".into())
        );
    }

    #[tokio::test]
    async fn inflate_message_with_final_block() {
        // "Hello" with a final block, from RFC 7692 section 7.2.3.4,
        // with and without the padding byte which is required but easy to miss.
        for payload in [
            &[0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x00][..],
            &[0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00][..],
        ] {
            let (mut ws, client) = server(&server_config(), false).await;
            let mut bytes = compressed(OpData::Text, payload);
            bytes.extend(compressed(OpData::Text, &compress_once(b"world")));
            let _client = send(client, bytes);

            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("Hello".into())
            );
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("world".into())
            );
        }
    }

    #[tokio::test]
    async fn rfc_examples() {
        // RFC 7692 section 7.2.3.1 and 7.2.3.2
        let (mut ws, client) = server(&server_config(), false).await;
        let hello = [0xf2, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00];
        let mut bytes = compressed(OpData::Text, &hello);
        // The second one refers to the first one.
        bytes.extend(compressed(OpData::Text, &[0xf2, 0x00, 0x11, 0x00, 0x00]));
        // Fragmented: section 7.2.3.1
        bytes.extend(client_frame(
            OpCode::Data(OpData::Text),
            false,
            true,
            Some(MASK),
            &[0xf2, 0x48, 0xcd],
        ));
        bytes.extend(client_frame(
            OpCode::Data(OpData::Continue),
            true,
            false,
            Some(MASK),
            &[0xc9, 0xc9, 0x07, 0x00],
        ));
        let _client = send(client, bytes);

        for _ in 0..3 {
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("Hello".into())
            );
        }
    }

    #[test]
    fn encode_with_all_levels_and_windows() {
        let data = "hello world, this is a test ".repeat(2000);
        for level in 0..=9 {
            for window_bits in MIN_WINDOW_BITS..=MAX_WINDOW_BITS {
                let mut negotiated = negotiated(false, false);
                negotiated.server_max_window_bits = window_bits;
                let config = PerMessageDeflateConfig::default().threshold(0).level(level);
                let mut deflater = Deflater::new(&config, &negotiated);
                // The window bits are what the client is required to accept.
                let mut client = Decompress::new_with_window_bits(false, window_bits);
                for _ in 0..2 {
                    let msg = Message::Text(data.clone().into());
                    let frame = unwrap_frame(deflater.encode(msg).unwrap());
                    assert_eq!(inflate(&mut client, frame.payload()), data.as_bytes());
                }
            }
        }
    }

    #[tokio::test]
    async fn inflate_with_restricted_window() {
        let (client, server_side) = duplex(1024);
        let mut negotiated = negotiated(false, false);
        negotiated.client_max_window_bits = 8;
        let stream = InflateStream::new(server_side, &server_config(), &negotiated);
        assert_eq!(stream.window_bits, MIN_WINDOW_BITS);
        let mut ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

        let data = "hello world, this is a test ".repeat(100);
        let mut compress = Compress::new_with_window_bits(Compression::default(), false, 9);
        let _client = send(
            client,
            compressed(OpData::Text, &deflate(&mut compress, data.as_bytes())),
        );
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            Message::Text(data.into())
        );
    }

    /// Expect the first message of the server to be an error.
    async fn read_err(config: &EngineIoConfig, bytes: Vec<u8>) -> tungstenite::Error {
        let (mut ws, client) = server(config, false).await;
        let _client = send(client, bytes);
        ws.next().await.unwrap().unwrap_err()
    }

    #[tokio::test]
    async fn decompression_bomb_is_rejected() {
        let mut config = server_config();
        config.ws_max_message_size = 1 << 20;

        let data = vec![0; 64 << 20];
        let payload = compress_once(&data);
        assert!(payload.len() < 128 * 1024, "len {}", payload.len());

        let (mut ws, client) = server(&config, false).await;
        let _client = send(client, compressed(OpData::Binary, &payload));
        let err = ws.next().await.unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                tungstenite::Error::Capacity(CapacityError::MessageTooLong { .. })
            ),
            "{err:?}"
        );
        // The message was not decompressed further than the limit.
        let total_out = ws.get_ref().inflater.as_ref().unwrap().total_out();
        assert!(
            total_out <= (1 << 20) + 2 * MAX_INFLATED_CHUNK_SIZE as u64,
            "{total_out}"
        );
    }

    #[tokio::test]
    async fn too_large_compressed_frame_is_rejected() {
        let mut config = server_config();
        config.ws_max_frame_size = 1024;
        let err = read_err(&config, compressed(OpData::Binary, &noise(2048))).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Capacity(CapacityError::MessageTooLong { .. })
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn corrupted_payload_is_rejected() {
        // A block with the reserved block type
        let bytes = compressed(OpData::Binary, &[0xff; 32]);
        match read_err(&server_config(), bytes).await {
            tungstenite::Error::Io(err) => assert_eq!(err.kind(), io::ErrorKind::InvalidData),
            err => panic!("{err:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_utf8_is_rejected() {
        let bytes = compressed(OpData::Text, &compress_once(&[0xff, 0xfe, 0xfd]));
        let err = read_err(&server_config(), bytes).await;
        assert!(matches!(err, tungstenite::Error::Utf8(_)), "{err:?}");
    }

    #[tokio::test]
    async fn unmasked_compressed_frame_is_rejected() {
        let payload = compress_once(b"hello");
        let bytes = client_frame(OpCode::Data(OpData::Text), true, true, None, &payload);
        let err = read_err(&server_config(), bytes).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::UnmaskedFrameFromClient)
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn rsv1_on_control_frame_is_rejected() {
        let bytes = client_frame(OpCode::Control(Control::Ping), true, true, Some(MASK), b"");
        let err = read_err(&server_config(), bytes).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::NonZeroReservedBits)
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn rsv1_on_continuation_frame_is_rejected() {
        let mut bytes = client_frame(OpCode::Data(OpData::Text), false, true, Some(MASK), &[]);
        bytes.extend(client_frame(
            OpCode::Data(OpData::Continue),
            true,
            true,
            Some(MASK),
            &compress_once(b"hello"),
        ));
        let err = read_err(&server_config(), bytes).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::NonZeroReservedBits)
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn new_message_during_a_compressed_message_is_rejected() {
        let mut bytes = client_frame(OpCode::Data(OpData::Text), false, true, Some(MASK), &[]);
        bytes.extend(client_frame(
            OpCode::Data(OpData::Text),
            true,
            false,
            Some(MASK),
            b"plain",
        ));
        let err = read_err(&server_config(), bytes).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::ExpectedFragment(OpData::Text))
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn invalid_opcode_is_rejected() {
        let err = read_err(&server_config(), vec![0x83, 0x80, 0, 0, 0, 0]).await;
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::InvalidOpcode(3))
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn eof_in_a_compressed_frame() {
        let mut bytes = compressed(OpData::Text, &compress_once(b"hello"));
        bytes.pop();
        let (mut ws, client) = server(&server_config(), false).await;
        let handle = send(client, bytes);
        drop(handle.await.unwrap());

        let err = ws.next().await.unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn writes_are_forwarded() {
        let (mut ws, mut client) = server(&server_config(), false).await;
        let mut deflater = deflater(0, false);
        let msg = deflater.encode(Message::Text("hello".into())).unwrap();
        futures_util::SinkExt::send(&mut ws, msg).await.unwrap();

        let mut buf = [0; 2];
        tokio::io::AsyncReadExt::read_exact(&mut client, &mut buf)
            .await
            .unwrap();
        // FIN + RSV1 + text opcode
        assert_eq!(buf[0], 0xC1);
    }
}
