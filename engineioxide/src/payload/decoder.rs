//! ## Decodes the payload stream into packets
//!
//! There is two versions of the decoder:
//! - v4_decoder: Decodes the payload stream according to the [engine.io v4 protocol](https://socket.io/fr/docs/v4/engine-io-protocol/#http-long-polling-1)
//! - v3_decoder: Decodes the payload stream according to the [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
//!

use futures::Stream;
use http::StatusCode;
use tracing::debug;

use crate::{errors::Error, packet::Packet};
use bytes::Buf;
use std::io::BufRead;

use super::buf::BufList;

struct Payload<B: http_body::Body> {
    body: B,
    buffer: BufList<B::Data>,
    end_of_stream: bool,
    current_payload_size: u64,

    #[cfg(feature = "v3")]
    yield_packets: u32,
}

impl<B: http_body::Body> Payload<B> {
    fn new(body: B) -> Self {
        Self {
            body,
            buffer: BufList::new(),
            end_of_stream: false,
            current_payload_size: 0,
            #[cfg(feature = "v3")]
            yield_packets: 0,
        }
    }
}

/// Polls the body stream for data and adds it to the chunk list in the state
/// Returns an error if the packet length exceeds the maximum allowed payload size
async fn poll_body(
    state: &mut Payload<impl http_body::Body<Error = impl std::fmt::Debug> + Unpin>,
    max_payload: u64,
) -> Result<(), Error> {
    match state.body.data().await.transpose() {
        Ok(Some(data)) if state.current_payload_size + (data.remaining() as u64) <= max_payload => {
            state.current_payload_size += data.remaining() as u64;
            state.buffer.push(data);
            Ok(())
        }
        Ok(Some(_)) => Err(Error::PayloadTooLarge),
        Ok(None) => {
            state.end_of_stream = true;
            Ok(())
        }
        Err(e) => {
            debug!("error reading body stream: {:?}", e);
            Err(Error::HttpErrorResponse(StatusCode::BAD_REQUEST))
        }
    }
}

#[cfg(feature = "v4")]
pub fn v4_decoder(
    body: impl http_body::Body<Error = impl std::fmt::Debug> + Unpin,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, Error>> {
    use super::PACKET_SEPARATOR_V4;
    debug!("decoding payload with v4 decoder");

    let state = Payload::new(body);

    futures::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream {
                if let Err(e) = poll_body(&mut state, max_payload).await {
                    break Some((Err(e), state));
                }
            }

            // Read from the buffer until the packet separator is found
            if let Err(e) = (&mut state.buffer)
                .reader()
                .read_until(PACKET_SEPARATOR_V4, &mut packet_buf)
            {
                break Some((Err(Error::Io(e)), state));
            }

            let separator_found = packet_buf.ends_with(&[PACKET_SEPARATOR_V4]);

            if separator_found {
                packet_buf.pop(); // Remove the separator from the packet buffer
            }

            // Check if a complete packet is found or reached end of stream with remaining data
            if separator_found
                || (state.end_of_stream && state.buffer.remaining() == 0 && !packet_buf.is_empty())
            {
                let packet = std::str::from_utf8(&packet_buf)
                    .map_err(|_| Error::InvalidPacketLength)
                    .and_then(Packet::try_from); // Convert the packet buffer to a Packet object
                break Some((packet, state)); // Emit the packet and the updated state
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None; // Reached end of stream with no more data, end the stream
            }
        }
    })
}

#[cfg(feature = "v3")]
pub fn v3_binary_decoder(
    body: impl http_body::Body<Error = impl std::fmt::Debug> + Unpin,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, Error>> {
    use std::io::Read;

    use crate::payload::{
        BINARY_PACKET_IDENTIFIER_V3, BINARY_PACKET_SEPARATOR_V3, STRING_PACKET_IDENTIFIER_V3,
    };

    let state = Payload::new(body);
    debug!("decoding payload with v3 binary decoder");

    futures::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_type: Option<u8> = None;
        let mut packet_size: u64 = 0;

        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream {
                if let Err(e) = poll_body(&mut state, max_payload).await {
                    break Some((Err(e), state));
                }
            }

            // If there is no packet_type found
            if packet_type.is_none() && state.buffer.remaining() > 0 {
                // Read from the buffer until the packet separator is found
                if let Err(e) = (&mut state.buffer)
                    .reader()
                    .read_until(BINARY_PACKET_SEPARATOR_V3, &mut packet_buf)
                {
                    break Some((Err(Error::Io(e)), state));
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
                        _ => break Some((Err(Error::InvalidPacketLength), state)),
                    }

                    if packet_buf.len() > 9 {
                        break Some((Err(Error::InvalidPacketLength), state));
                    }

                    let size_str = &packet_buf[1..]
                        .iter()
                        .map(|b| (b + 48) as char)
                        .collect::<String>();
                    if let Ok(size) = size_str.parse() {
                        packet_size = size;
                    } else {
                        break Some((Err(Error::InvalidPacketLength), state));
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
                    STRING_PACKET_IDENTIFIER_V3 => std::str::from_utf8(&packet_buf)
                        .map_err(|_| Error::InvalidPacketLength)
                        .and_then(Packet::try_from), // Convert the packet buffer to a Packet object
                    BINARY_PACKET_IDENTIFIER_V3 => Ok(Packet::BinaryV3(packet_buf)),
                    _ => Err(Error::InvalidPacketLength),
                };

                break Some((packet, state));
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None;
            }
        }
    })
}

#[cfg(feature = "v3")]
pub fn v3_string_decoder(
    body: impl http_body::Body<Error = impl std::fmt::Debug> + Unpin,
    max_payload: u64,
) -> impl Stream<Item = Result<Packet, Error>> {
    use std::io::ErrorKind;
    use unicode_segmentation::UnicodeSegmentation;

    use crate::payload::STRING_PACKET_SEPARATOR_V3;

    debug!("decoding payload with v3 string decoder");
    let state = Payload::new(body);

    futures::stream::unfold(state, move |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_graphemes_len: usize = 0;
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream {
                if let Err(e) = poll_body(&mut state, max_payload).await {
                    break Some((Err(e), state));
                }
            }
            if state.end_of_stream && state.buffer.remaining() == 0 && state.yield_packets > 0 {
                break None; // Reached end of stream with no more data, end the stream
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                return Some((Err(Error::InvalidPacketLength), state));
            }

            let mut reader = (&mut state.buffer).reader();

            // Read the packet length from the buffer
            if packet_graphemes_len == 0 {
                loop {
                    let (done, used) = {
                        let remaining = reader.get_ref().remaining();
                        let available = match reader.fill_buf() {
                            Ok(n) => n,
                            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(e) => return Some((Err(Error::Io(e)), state)),
                        };
                        let old_len = packet_buf.len();
                        packet_buf.extend_from_slice(available);
                        // Find the position of the packet separator
                        match memchr::memchr(STRING_PACKET_SEPARATOR_V3, &packet_buf) {
                            Some(i) => {
                                // Extract the packet length from the available data
                                packet_graphemes_len = match std::str::from_utf8(&packet_buf[..i])
                                    .map_err(|_| Error::InvalidPacketLength)
                                    .and_then(|s| {
                                        s.parse::<usize>().map_err(|_| Error::InvalidPacketLength)
                                    }) {
                                    Ok(size) => size,
                                    Err(e) => return Some((Err(e), state)),
                                };
                                packet_buf.clear();

                                (true, i + 1 - old_len) // Mark as done and set the used bytes count
                            }
                            None if state.end_of_stream && remaining - available.len() == 0 => {
                                return None;
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

            if packet_graphemes_len == 0 {
                continue; // No packet length, continue to read more data
            }

            // Read bytes from the buffer until the packet length is reached

            // Read the next chunk of data from the chunk list
            let data: &[u8] = reader.fill_buf().unwrap();

            let old_len = packet_buf.len();
            packet_buf.extend_from_slice(data);
            let byte_read = match std::str::from_utf8(&packet_buf) {
                Ok(fulldata) => {
                    let i = fulldata
                        .grapheme_indices(true)
                        .nth(packet_graphemes_len)
                        .map(|(i, _)| i);
                    if let Some(i) = i {
                        packet_buf.truncate(i);
                        packet_buf.len() - old_len
                    } else {
                        data.len()
                    }
                }
                Err(e) => {
                    let chunk =
                        unsafe { std::str::from_utf8_unchecked(&packet_buf[..e.valid_up_to()]) };
                    let i = chunk
                        .grapheme_indices(true)
                        .nth(packet_graphemes_len)
                        .map(|(i, _)| i);
                    if let Some(i) = i {
                        packet_buf.truncate(i);
                        packet_buf.len() - old_len
                    } else {
                        data.len()
                    }
                }
            };
            reader.consume(byte_read);

            // Check if the packet length matches the number of characters
            if let Ok(packet) = std::str::from_utf8(&packet_buf) {
                if packet.graphemes(true).count() == packet_graphemes_len {
                    let packet = Packet::try_from(packet).map_err(|_| Error::InvalidPacketLength);
                    state.yield_packets += 1;
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
    use futures::StreamExt;
    use http_body::Full;

    use crate::packet::Packet;

    use super::*;

    const MAX_PAYLOAD: u64 = 100_000;

    #[cfg(feature = "v4")]
    #[tokio::test]
    async fn payload_iterator_v4() {
        assert!(cfg!(feature = "v4"));
        let data = Full::new(Bytes::from("4foo\x1e4€f\x1e4f"));
        let payload = v4_decoder(data, MAX_PAYLOAD);
        futures::pin_mut!(payload);
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
        assert_eq!(payload.next().await.is_none(), true);
    }

    #[cfg(feature = "v4")]
    #[tokio::test]
    async fn payload_stream_v4() {
        assert!(cfg!(feature = "v4"));
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        for i in 1..DATA.len() {
            println!("payload stream v4 chunk size: {i}");
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v4_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
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
            assert_eq!(payload.next().await.is_none(), true);
        }
    }

    #[cfg(feature = "v4")]
    #[tokio::test]
    async fn max_payload_v4() {
        assert!(cfg!(feature = "v4"));
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        const MAX_PAYLOAD: u64 = 3;
        for i in 1..DATA.len() {
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v4_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(packet, Err(Error::PayloadTooLarge)));
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_iterator_v3() {
        assert!(cfg!(feature = "v3"));

        let data = Full::new(Bytes::from("4:4foo3:4€f10:4faaaaaaaaa"));
        let payload = v3_string_decoder(data, MAX_PAYLOAD);
        futures::pin_mut!(payload);
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
            Packet::Message(msg) if msg == "faaaaaaaa"
        ));
        assert_eq!(payload.next().await.is_none(), true);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn binary_payload_iterator_v3() {
        assert!(cfg!(feature = "v3"));

        const PAYLOAD: &'static [u8] = &[
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        const BINARY_PAYLOAD: &'static [u8] = &[1, 2, 3, 4];
        let data = Full::new(Bytes::from(PAYLOAD));
        let payload = v3_binary_decoder(data, MAX_PAYLOAD);
        futures::pin_mut!(payload);
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::Message(msg) if msg == "hello€"
        ));
        assert!(matches!(
            payload.next().await.unwrap().unwrap(),
            Packet::BinaryV3(msg) if msg == BINARY_PAYLOAD
        ));
        assert_eq!(payload.next().await.is_none(), true);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn string_payload_stream_v3() {
        assert!(cfg!(feature = "v3"));
        const DATA: &[u8] = "4:4foo3:4€f11:4baaaaaaaar".as_bytes();
        for i in 1..DATA.len() {
            println!("payload stream v3 chunk size: {i}");
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_string_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
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
            assert_eq!(payload.next().await.is_none(), true);
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn binary_payload_stream_v3() {
        assert!(cfg!(feature = "v3"));

        const PAYLOAD: &'static [u8] = &[
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        const BINARY_PAYLOAD: &'static [u8] = &[1, 2, 3, 4];

        for i in 1..PAYLOAD.len() {
            println!("payload stream v3 chunk size: {i}");
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                PAYLOAD.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_binary_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::Message(msg) if msg == "hello€"
            ));
            assert!(matches!(
                payload.next().await.unwrap().unwrap(),
                Packet::BinaryV3(msg) if msg == BINARY_PAYLOAD
            ));
            assert_eq!(payload.next().await.is_none(), true);
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn max_payload_v3() {
        assert!(cfg!(feature = "v3"));
        const DATA: &[u8] = "4:4foo3:4€f11:4baaaaaaaar".as_bytes();
        const MAX_PAYLOAD: u64 = 3;
        for i in 1..DATA.len() {
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_binary_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(packet, Err(Error::PayloadTooLarge)));
        }
        for i in 1..DATA.len() {
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = v3_string_decoder(stream, MAX_PAYLOAD);
            futures::pin_mut!(payload);
            let packet = payload.next().await.unwrap();
            assert!(matches!(packet, Err(Error::PayloadTooLarge)));
        }
    }
}
