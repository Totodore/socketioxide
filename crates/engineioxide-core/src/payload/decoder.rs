//! ## Decodes the payload stream into packets
//!
//! There are two versions of the decoder:
//! - v4_decoder: Decodes the payload stream according to the [engine.io v4 protocol](https://socket.io/fr/docs/v4/engine-io-protocol/#http-long-polling-1)
//! - v3_decoder: Decodes the payload stream according to the [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
//!

use crate::{Packet, PacketParseError, ProtocolVersion};
use futures_util::{Stream, StreamExt};

use bytes::Buf;
use http_body::Body;
use http_body_util::BodyStream;
use std::io::BufRead;

use super::buf::BufList;

struct Payload<B: Body + Unpin> {
    body: BodyStream<B>,
    buffer: BufList<B::Data>,
    end_of_stream: bool,
    current_payload_size: u64,
}

impl<B: Body + Unpin> Payload<B> {
    fn new(body: B) -> Self {
        Self {
            body: BodyStream::new(body),
            buffer: BufList::new(),
            end_of_stream: false,
            current_payload_size: 0,
        }
    }
}

/// Polls the body stream for data and adds it to the chunk list in the state
/// Returns an error if the packet length exceeds the maximum allowed payload size
async fn poll_body<B, E>(state: &mut Payload<B>, max_payload: u64) -> Result<(), PacketParseError>
where
    B: Body<Error = E> + Unpin,
    E: std::fmt::Debug,
{
    let data = match state.body.next().await.transpose() {
        Ok(Some(frame)) if frame.is_data() => Ok(frame
            .into_data()
            .unwrap_or_else(|_| unreachable!("frame.is_data() is true"))),
        // None or Trailer frames -> ignore and EOS
        Ok(_) => {
            state.end_of_stream = true;
            return Ok(());
        }
        Err(_e) => {
            #[cfg(feature = "tracing")]
            tracing::debug!("error reading body stream: {:?}", _e);
            Err(PacketParseError::InvalidPacketPayload)
        }
    }?;
    if state.current_payload_size + (data.remaining() as u64) <= max_payload {
        state.current_payload_size += data.remaining() as u64;
        state.buffer.push(data);
        Ok(())
    } else {
        Err(PacketParseError::PayloadTooLarge { max: max_payload })
    }
}

pub fn v4_decoder<B, E>(
    body: B,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, PacketParseError>>
where
    B: Body<Error = E> + Unpin,
    E: std::fmt::Debug,
{
    use super::PACKET_SEPARATOR_V4;
    #[cfg(feature = "tracing")]
    tracing::debug!("decoding payload with v4 decoder");

    let state = Payload::new(body);

    futures_util::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream
                && let Err(e) = poll_body(&mut state, max_payload).await
            {
                break Some((Err(e), state));
            }

            // Read from the buffer until the packet separator is found
            if let Err(_err) = (&mut state.buffer)
                .reader()
                .read_until(PACKET_SEPARATOR_V4, &mut packet_buf)
            {
                #[cfg(feature = "tracing")]
                tracing::debug!("failed to read packet payload: {_err}");

                break Some((Err(PacketParseError::InvalidPacketPayload), state));
            }

            let separator_found = packet_buf.ends_with(&[PACKET_SEPARATOR_V4]);

            if separator_found {
                packet_buf.pop(); // Remove the separator from the packet buffer
            }

            // Check if a complete packet is found or reached end of stream with remaining data
            if separator_found
                || (state.end_of_stream && state.buffer.remaining() == 0 && !packet_buf.is_empty())
            {
                let packet = String::from_utf8(packet_buf)
                    .map_err(PacketParseError::from)
                    .and_then(|v| Packet::parse(ProtocolVersion::V4, v)); // Convert the packet buffer to a Packet object
                break Some((packet, state)); // Emit the packet and the updated state
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None; // Reached end of stream with no more data, end the stream
            }
        }
    })
}

#[cfg(feature = "v3")]
pub fn v3_binary_decoder<B, E>(
    body: B,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, PacketParseError>>
where
    B: Body<Error = E> + Unpin,
    E: std::fmt::Debug,
{
    use std::io::Read;

    use crate::payload::{
        BINARY_PACKET_IDENTIFIER_V3, BINARY_PACKET_SEPARATOR_V3, STRING_PACKET_IDENTIFIER_V3,
    };

    let state = Payload::new(body);
    #[cfg(feature = "tracing")]
    tracing::debug!("decoding payload with v3 binary decoder");

    futures_util::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_type: Option<u8> = None;
        let mut packet_size: u64 = 0;

        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream
                && let Err(e) = poll_body(&mut state, max_payload).await
            {
                break Some((Err(e), state));
            }

            // If there is no packet_type found
            if packet_type.is_none() && state.buffer.remaining() > 0 {
                // Read from the buffer until the packet separator is found
                if let Err(_err) = (&mut state.buffer)
                    .reader()
                    .read_until(BINARY_PACKET_SEPARATOR_V3, &mut packet_buf)
                {
                    #[cfg(feature = "tracing")]
                    tracing::debug!("failed to read packet payload: {_err}");

                    break Some((Err(PacketParseError::InvalidPacketPayload), state));
                }

                // Extract packet_type and packet_size
                if packet_buf.ends_with(&[BINARY_PACKET_SEPARATOR_V3]) {
                    packet_buf.pop();
                    match packet_buf.first() {
                        Some(&BINARY_PACKET_IDENTIFIER_V3) => {
                            packet_type = Some(BINARY_PACKET_IDENTIFIER_V3)
                        }
                        Some(&STRING_PACKET_IDENTIFIER_V3) => {
                            packet_type = Some(STRING_PACKET_IDENTIFIER_V3)
                        }
                        _ => break Some((Err(PacketParseError::InvalidPacketLen), state)),
                    }

                    if packet_buf.len() > 9 {
                        break Some((Err(PacketParseError::InvalidPacketLen), state));
                    }

                    let size_str = &packet_buf[1..]
                        .iter()
                        .map(|b| (b.wrapping_add(b'0')) as char)
                        .collect::<String>();
                    if let Ok(size) = size_str.parse() {
                        packet_size = size;
                    } else {
                        break Some((Err(PacketParseError::InvalidPacketLen), state));
                    }
                    packet_buf.clear();
                }
            } else if packet_size > 0 && state.buffer.remaining() >= packet_size as usize {
                // If the packet_type is found and there is enough bytes available

                // Read packet data
                let mut reader = (&mut state.buffer).reader().take(packet_size);
                // In case of BINARY_PACKET we should skip MESSAGE type provided
                if packet_type.unwrap() == BINARY_PACKET_IDENTIFIER_V3 {
                    reader.consume(1);
                }
                reader.read_to_end(&mut packet_buf).unwrap();
                // Read the packet data
                let packet = match packet_type.unwrap() {
                    STRING_PACKET_IDENTIFIER_V3 => String::from_utf8(packet_buf)
                        .map_err(PacketParseError::from)
                        .and_then(|v| Packet::parse(ProtocolVersion::V3, v)), // Convert the packet buffer to a Packet object
                    BINARY_PACKET_IDENTIFIER_V3 => Ok(Packet::BinaryV3(packet_buf.into())),
                    _ => Err(PacketParseError::InvalidPacketLen),
                };

                break Some((packet, state));
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None;
            } else if state.end_of_stream {
                // EOS reached with leftover bytes that cannot form a complete
                // packet (truncated header or truncated body).
                break Some((Err(PacketParseError::InvalidPacketLen), state));
            }
        }
    })
}

/// Return the byte offset in `s` reached after exactly `target` UTF-16 code units.
/// Returns `None` if `s` has fewer UTF-16 code units than requested.
#[cfg(feature = "v3")]
fn utf16_byte_offset(s: &str, target: usize) -> Option<usize> {
    if target == 0 {
        return Some(0);
    }
    let mut count = 0usize;
    for (i, ch) in s.char_indices() {
        count += ch.len_utf16();
        if count >= target {
            // If a surrogate-pair char straddles the boundary (count > target),
            // we still cut after the char — the length check below will reject it.
            return Some(i + ch.len_utf8());
        }
    }
    None
}

#[cfg(feature = "v3")]
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(feature = "v3")]
pub fn v3_string_decoder(
    body: impl Body<Error = impl std::fmt::Debug> + Unpin,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, PacketParseError>> {
    use std::io::ErrorKind;

    use crate::payload::STRING_PACKET_SEPARATOR_V3;

    #[cfg(feature = "tracing")]
    tracing::debug!("decoding payload with v3 string decoder");
    let state = Payload::new(body);

    futures_util::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_utf16_len: usize = 0;
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream
                && let Err(e) = poll_body(&mut state, max_payload).await
            {
                break Some((Err(e), state));
            }
            if state.end_of_stream && state.buffer.remaining() == 0 {
                break None; // Reached end of stream with no more data, end the stream
            }

            let mut reader = (&mut state.buffer).reader();

            // Read the packet length from the buffer
            if packet_utf16_len == 0 {
                loop {
                    let (done, used) = {
                        let remaining = reader.get_ref().remaining();
                        let available = match reader.fill_buf() {
                            Ok(n) => n,
                            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(_err) => {
                                #[cfg(feature = "tracing")]
                                tracing::debug!("failed to read packet payload: {_err}");

                                return Some((Err(PacketParseError::InvalidPacketPayload), state));
                            }
                        };
                        let old_len = packet_buf.len();
                        packet_buf.extend_from_slice(available);
                        // Find the position of the packet separator
                        match memchr::memchr(STRING_PACKET_SEPARATOR_V3, &packet_buf) {
                            Some(i) => {
                                // Extract the packet length from the available data
                                packet_utf16_len = match std::str::from_utf8(&packet_buf[..i])
                                    .map_err(PacketParseError::from)
                                    .and_then(|s| {
                                        s.parse::<usize>()
                                            .map_err(|_| PacketParseError::InvalidPacketLen)
                                    }) {
                                    Ok(size) => size,
                                    Err(e) => return Some((Err(e), state)),
                                };
                                packet_buf.clear();

                                (true, i + 1 - old_len) // Mark as done and set the used bytes count
                            }
                            None if state.end_of_stream && remaining - available.len() == 0 => {
                                return Some((Err(PacketParseError::InvalidPacketLen), state));
                            } // Reached end of stream and end of bufferered chunks without finding the separator
                            None => (false, available.len()), // Continue reading more data
                        }
                    };
                    reader.consume(used); // Consume the used bytes from the buffer
                    if done || used == 0 {
                        break; // Break the loop if done or no more data used
                    }
                }
            }

            if packet_utf16_len == 0 {
                continue; // No packet length, continue to read more data
            }

            // Read bytes from the buffer until the packet length is reached

            // Read the next chunk of data from the chunk list
            let data: &[u8] = reader.fill_buf().unwrap(); // fill_buf is infallible.

            let old_len = packet_buf.len();
            packet_buf.extend_from_slice(data);
            let truncate_to = |s: &str| utf16_byte_offset(s, packet_utf16_len);

            // try to decode the packet buf, if there is still not enough data we consume
            // everything and continue polling.
            let byte_read = match std::str::from_utf8(&packet_buf) {
                Ok(fulldata) => match truncate_to(fulldata) {
                    Some(i) => {
                        packet_buf.truncate(i);
                        packet_buf.len() - old_len
                    }
                    None => data.len(), // not enough data, we consume everything
                },
                Err(e) => {
                    // packet buf is in the middle of a char boundary. We get the available data and try to extract
                    // packets.

                    // SAFETY: packet_buf is a valid utf8 string checkd above
                    let chunk =
                        unsafe { std::str::from_utf8_unchecked(&packet_buf[..e.valid_up_to()]) };
                    match truncate_to(chunk) {
                        Some(i) => {
                            packet_buf.truncate(i);
                            packet_buf.len() - old_len
                        }
                        None => data.len(),
                    }
                }
            };
            reader.consume(byte_read);

            // Check if the packet length matches the number of UTF-16 code units
            if let Ok(packet) = std::str::from_utf8(&packet_buf) {
                if utf16_len(packet) == packet_utf16_len {
                    // SAFETY: packet_buf is a valid utf8 string checkd above
                    let packet = unsafe { String::from_utf8_unchecked(packet_buf) };
                    let packet = Packet::parse(ProtocolVersion::V3, packet)
                        .map_err(|_| PacketParseError::InvalidPacketLen);
                    break Some((packet, state)); // Emit the packet and the updated state
                }
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None; // Reached end of stream with no more data, end the stream
            }
        }
    })
}

#[cfg(test)]
mod tests {

    use bytes::Bytes;
    use futures_util::StreamExt;
    use http_body::Frame;
    use http_body_util::{Full, StreamBody};

    use super::*;

    const MAX_PAYLOAD: u64 = 100_000;

    #[tokio::test]
    async fn payload_iterator_v4() {
        let data = Full::new(Bytes::from("4foo\x1e4€f\x1e4f"));
        let payload = v4_decoder(data, MAX_PAYLOAD);
        futures_util::pin_mut!(payload);
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "foo"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "€f"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "f"
        ));
        assert!(payload.next().await.is_none());
    }

    #[tokio::test]
    async fn payload_stream_v4() {
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v4_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "foo"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "€f"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "fo"
            ));
            assert!(payload.next().await.is_none());
        }
    }

    #[tokio::test]
    async fn max_payload_v4() {
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        const MAX_PAYLOAD: u64 = 3;
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v4_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(
                packet,
                Err(PacketParseError::PayloadTooLarge { max: MAX_PAYLOAD })
            ));
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_v3_utf16_length() {
        // The wire length is the number of UTF-16 code units (JS-style).
        // Two packets: "4𝕊" → 3 UTF-16 code units ('4' + surrogate pair),
        // then "4ab" → 3 UTF-16 code units.
        let data = Full::new(Bytes::from("3:4𝕊3:4ab"));
        let payload = v3_string_decoder(data, MAX_PAYLOAD);
        futures_util::pin_mut!(payload);
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "𝕊"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "ab"
        ));
        assert!(payload.next().await.is_none());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_iterator_v3() {
        let data = Full::new(Bytes::from("4:4foo3:4€f11:4faaaaaaaaa"));
        let payload = v3_string_decoder(data, MAX_PAYLOAD);
        futures_util::pin_mut!(payload);
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "foo"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "€f"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "faaaaaaaaa"
        ));
        assert!(payload.next().await.is_none());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn binary_payload_iterator_v3() {
        const PAYLOAD: &[u8] = &[
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        const BINARY_PAYLOAD: &[u8] = &[1, 2, 3, 4];
        let data = Full::new(Bytes::from(PAYLOAD));
        let payload = v3_binary_decoder(data, MAX_PAYLOAD);
        futures_util::pin_mut!(payload);
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "hello€"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::BinaryV3(msg) if msg == BINARY_PAYLOAD
        ));
        assert!(payload.next().await.is_none());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_stream_v3() {
        const DATA: &[u8] = "4:4foo3:4€f11:4baaaaaaaar".as_bytes();
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_string_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            let packet = payload.next().await.unwrap().unwrap();
            assert!(matches!(
                packet,
                Packet::Message(msg) if msg == "foo"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "€f"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "baaaaaaaar"
            ));
            assert!(payload.next().await.is_none());
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn binary_payload_stream_v3() {
        const PAYLOAD: &[u8] = &[
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        const BINARY_PAYLOAD: &[u8] = &[1, 2, 3, 4];

        for i in 1..PAYLOAD.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                PAYLOAD
                    .chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_binary_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "hello€"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::BinaryV3(msg) if msg == BINARY_PAYLOAD
            ));
            assert!(payload.next().await.is_none());
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn binary_payload_truncated_v3() {
        // Header declares a 5-byte (type byte + 4 payload bytes) binary
        // packet but the body ends after only 2 payload bytes. The decoder
        // must terminate (with an error or end-of-stream) rather than spin
        // forever waiting on data that will never arrive.
        const TRUNCATED: &[u8] = &[1, 5, 255, 4, 1, 2];
        let data = Full::new(Bytes::from(TRUNCATED));
        let payload = v3_binary_decoder(data, MAX_PAYLOAD);
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            futures_util::pin_mut!(payload);
            payload.next().await
        })
        .await
        .expect("v3_binary_decoder hung on truncated packet");
        assert!(matches!(result, Some(Err(_)) | None));
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn max_payload_v3() {
        const DATA: &[u8] = "4:4foo3:4€f11:4baaaaaaaar".as_bytes();
        const MAX_PAYLOAD: u64 = 3;
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_binary_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(
                packet,
                Err(PacketParseError::PayloadTooLarge { max: MAX_PAYLOAD })
            ));
        }
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_string_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(
                packet,
                Err(PacketParseError::PayloadTooLarge { max: MAX_PAYLOAD })
            ));
        }
    }

    // (1) Same payload as `string_payload_v3_utf16_length`, but fed through the
    // stream in chunks of every size. This exercises the `Err(valid_up_to())`
    // path where the 4-byte `𝕊` (2 UTF-16 code units) is split across chunk
    // boundaries, combined with the UTF-16 length accounting and truncation.
    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_stream_v3_utf16_length() {
        const DATA: &[u8] = "3:4𝕊3:4ab".as_bytes();
        for i in 1..DATA.len() {
            let stream = StreamBody::new(futures_util::stream::iter(
                DATA.chunks(i)
                    .map(Frame::data)
                    .map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_string_decoder(stream, MAX_PAYLOAD);
            futures_util::pin_mut!(payload);
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "𝕊"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "ab"
            ));
            assert!(payload.next().await.is_none());
        }
    }

    // (2) The declared length cuts in the middle of a surrogate pair: the
    // content "4𝕊" is 3 UTF-16 code units but the header declares 2, landing
    // the boundary between the two code units of `𝕊`. `utf16_byte_offset` cuts
    // after the whole char (count > target), so the UTF-16 length check must
    // reject the packet rather than mis-decode it or loop forever.
    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_v3_surrogate_straddle_rejected() {
        let data = Full::new(Bytes::from("2:4𝕊"));
        let payload = v3_string_decoder(data, MAX_PAYLOAD);
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            futures_util::pin_mut!(payload);
            payload.next().await
        })
        .await
        .expect("v3_string_decoder hung on surrogate-straddling length");
        assert!(matches!(result, Some(Err(_)) | None));
    }

    // (3) Direct unit tests for the UTF-16 helpers.
    #[cfg(feature = "v3")]
    #[test]
    fn utf16_byte_offset_unit() {
        // target 0 always yields offset 0
        assert_eq!(utf16_byte_offset("", 0), Some(0));
        assert_eq!(utf16_byte_offset("abc", 0), Some(0));

        // pure ASCII: 1 byte == 1 UTF-16 code unit
        assert_eq!(utf16_byte_offset("abc", 2), Some(2));
        assert_eq!(utf16_byte_offset("abc", 3), Some(3));
        assert_eq!(utf16_byte_offset("abc", 5), None); // too short

        // `€` is 3 UTF-8 bytes but a single UTF-16 code unit
        assert_eq!(utf16_byte_offset("€", 1), Some(3));

        // `𝕊` is 4 UTF-8 bytes and 2 UTF-16 code units
        assert_eq!(utf16_byte_offset("𝕊", 2), Some(4));
        // straddle: asking for 1 code unit still cuts after the whole char
        assert_eq!(utf16_byte_offset("𝕊", 1), Some(4));

        // mixed: '4' (1 unit) + `𝕊` (2 units)
        assert_eq!(utf16_byte_offset("4𝕊", 3), Some(5));
        assert_eq!(utf16_byte_offset("4𝕊", 2), Some(5)); // straddle past target
        assert_eq!(utf16_byte_offset("4𝕊", 10), None); // too short
    }

    // (4) The header declares a 10 UTF-16 code unit packet but the body ends
    // after only 3 ("4ab"). The decoder must terminate (error or end-of-stream)
    // rather than spin forever waiting on data that will never arrive. This is
    // the string-side analog of `binary_payload_truncated_v3`.
    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_truncated_v3() {
        let data = Full::new(Bytes::from("10:4ab"));
        let payload = v3_string_decoder(data, MAX_PAYLOAD);
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            futures_util::pin_mut!(payload);
            payload.next().await
        })
        .await
        .expect("v3_string_decoder hung on truncated packet");
        assert!(matches!(result, Some(Err(_)) | None));
    }
}
