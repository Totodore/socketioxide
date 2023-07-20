use futures::Stream;

use crate::{errors::Error, packet::Packet, service::ProtocolVersion};
use bytes::Buf;
use std::io::BufRead;

use self::buf::BufList;

#[cfg(feature = "v3")]
use unicode_segmentation::UnicodeSegmentation;

mod buf;
pub const PACKET_SEPARATOR_V4: u8 = b'\x1e';
pub const PACKET_SEPARATOR_V3: u8 = b':';

struct Payload<B: http_body::Body> {
    body: B,
    buffer: BufList<B::Data>,
    end_of_stream: bool,
}
#[cfg(feature = "v4")]
fn body_parser_v4(body: impl http_body::Body + Unpin) -> impl Stream<Item = Result<Packet, Error>> {
    let state = Payload {
        body,
        buffer: BufList::new(),
        end_of_stream: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream {
                match state.body.data().await.transpose() {
                    Ok(Some(data)) => state.buffer.push(data),
                    Ok(None) => state.end_of_stream = true,
                    Err(_) => todo!(), // Handle the error case
                }
            }

            // Read from the buffer until the packet separator is found
            (&mut state.buffer)
                .reader()
                .read_until(PACKET_SEPARATOR_V4, &mut packet_buf)
                .unwrap();

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
fn body_parser_v3(body: impl http_body::Body + Unpin) -> impl Stream<Item = Result<Packet, Error>> {
    use std::io::ErrorKind;

    let state = Payload {
        body,
        buffer: BufList::new(),
        end_of_stream: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_graphemes_len: usize = 0;
        loop {
            // Read data from the body stream into the buffer
            if !state.end_of_stream {
                match state.body.data().await.transpose() {
                    Ok(Some(data)) => state.buffer.push(data),
                    Ok(None) => state.end_of_stream = true,
                    Err(_) => todo!(), // Handle the error case
                }
            }
            let mut reader = (&mut state.buffer).reader();

            // Read the packet length from the buffer
            if packet_graphemes_len == 0 {
                loop {
                    let (done, used) = {
                        let available = match reader.fill_buf() {
                            Ok(n) => n,
                            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(e) => return Some((Err(Error::Io(e)), state)),
                        };
                        let old_len = packet_buf.len();
                        packet_buf.extend_from_slice(available);
                        // Find the position of the packet separator
                        match memchr::memchr(PACKET_SEPARATOR_V3, &packet_buf) {
                            Some(i) => {
                                // Extract the packet length from the available data
                                packet_graphemes_len = match std::str::from_utf8(&packet_buf[..i])
                                    .map_err(|_| Error::InvalidPacketLength)
                                    .and_then(|s| {
                                        s.parse::<usize>().map_err(|_| Error::InvalidPacketLength)
                                    }) {
                                    Ok(size) => size,
                                    Err(_) => {
                                        return Some((Err(Error::InvalidPacketLength), state))
                                    }
                                };
                                dbg!(std::str::from_utf8(&packet_buf));
                                packet_buf.clear();
                                dbg!(std::str::from_utf8(&packet_buf));

                                dbg!((true, i + 1 - old_len)) // Mark as done and set the used bytes count
                            }
                            None if state.end_of_stream && remaining - available.len() == 0 => {
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
            let data: &[u8] = reader.fill_buf().unwrap();

            let old_len = packet_buf.len();
            dbg!(data.len());
            packet_buf.extend_from_slice(data);

            let byte_read = match std::str::from_utf8(&packet_buf) {
                Ok(fulldata) => {
                    let i = dbg!(fulldata)
                        .grapheme_indices(true)
                        .nth(dbg!(packet_graphemes_len))
                        .map(|(i, _)| i);
                    if let Some(i) = i {
                        dbg!(std::str::from_utf8(&packet_buf));
                        packet_buf.truncate(i);
                        dbg!(std::str::from_utf8(&packet_buf));
                        packet_buf.len() - old_len
                    } else {
                        data.len()
                    }
                }
                Err(e) => {
                    let chunk = unsafe { std::str::from_utf8_unchecked(&data[..e.valid_up_to()]) };
                    let i = chunk
                        .char_indices()
                        .nth(packet_graphemes_len)
                        .map(|(i, _)| i);
                    if let Some(i) = i {
                        dbg!(&packet_buf);
                        packet_buf.truncate(i);
                        dbg!(&packet_buf);
                        packet_buf.len() - old_len
                    } else {
                        data.len()
                    }
                }
            };

            reader.consume(byte_read);
            let packet_str = std::str::from_utf8(&packet_buf);
            // Check if the packet length matches the number of characters
            } else if state.end_of_stream && state.buffer.remaining() == 0 {
                break None; // Reached end of stream with no more data, end the stream
            }
        }
    })
}
pub fn body_parser(
    body: impl http_body::Body + Unpin,
    #[allow(unused_variables)] protocol: ProtocolVersion,
) -> impl Stream<Item = Result<Packet, Error>> {
    #[cfg(all(feature = "v3", feature = "v4"))]
    {
        match protocol {
            ProtocolVersion::V4 => futures::future::Either::Left(body_parser_v4(body)),
            ProtocolVersion::V3 => futures::future::Either::Right(body_parser_v3(body)),
        }
    }
    #[cfg(all(feature = "v3", not(feature = "v4")))]
    {
        body_parser_v3(body)
    }
    #[cfg(all(feature = "v4", not(feature = "v3")))]
    {
        body_parser_v4(body)
    }
}

#[cfg(test)]
mod tests {

    use bytes::Bytes;
    use futures::StreamExt;
    use http_body::Full;

    use crate::packet::Packet;

    use super::*;
    #[cfg(feature = "v4")]
    #[tokio::test]
    async fn test_payload_iterator_v4() {
        assert!(cfg!(feature = "v4"));
        let data = Full::new(Bytes::from("4foo\x1e4€f\x1e4f"));
        let payload = body_parser_v4(data);
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
    async fn test_payload_stream_v4() {
        assert!(cfg!(feature = "v4"));
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        for i in 1..DATA.len() {
            println!("payload stream v4 chunk size: {i}");
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = body_parser_v4(stream);
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

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn test_payload_iterator_v3() {
        assert!(cfg!(feature = "v3"));

        let data = Full::new(Bytes::from("4:4foo3:4€f10:4faaaaaaaaa"));
        let payload = body_parser_v3(data);
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
    async fn test_payload_stream_v3() {
        assert!(cfg!(feature = "v3"));
        const DATA: &[u8] = "4:4foo3:4€f11:4baaaaaaaar".as_bytes();
        for i in 1..DATA.len() {
            println!("payload stream v3 chunk size: {i}");
            let stream = hyper::Body::wrap_stream(futures::stream::iter(
                DATA.chunks(i).map(Ok::<_, std::convert::Infallible>),
            ));
            let payload = body_parser_v3(stream);
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
}
