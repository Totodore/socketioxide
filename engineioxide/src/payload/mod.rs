use futures::Stream;

use crate::{errors::Error, packet::Packet, service::ProtocolVersion};
use bytes::Buf;
use std::io::BufRead;

use self::buf::BufList;

mod buf;
pub const PACKET_SEPARATOR_V4: u8 = b'\x1e';
pub const PACKET_SEPARATOR_V3: u8 = b':';

struct Payload<B: http_body::Body> {
    body: B,
    buffer: BufList<B::Data>,
}
#[cfg(feature = "v4")]
fn body_parser_v4(body: impl http_body::Body + Unpin) -> impl Stream<Item = Result<Packet, Error>> {
    let state = Payload {
        body,
        buffer: BufList::new(),
    };

    futures::stream::unfold(state, |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut end_of_stream = false;
        loop {
            if !end_of_stream {
                match state.body.data().await.transpose() {
                    Ok(Some(data)) => state.buffer.push(data),
                    Ok(None) => end_of_stream = true,
                    Err(_) => todo!(),
                }
            }

            (&mut state.buffer)
                .reader()
                .read_until(PACKET_SEPARATOR_V4, &mut packet_buf)
                .unwrap();
            let separator_found = packet_buf.ends_with(&[PACKET_SEPARATOR_V4]);
            if separator_found {
                packet_buf.pop();
            }
            if separator_found
                || (end_of_stream && state.buffer.remaining() == 0 && !packet_buf.is_empty())
            {
                let packet = std::str::from_utf8(&packet_buf)
                    .map_err(|_| Error::InvalidPacketLength)
                    .and_then(Packet::try_from);
                break Some((packet, state));
            } else if end_of_stream && state.buffer.remaining() == 0 {
                break None;
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
    };

    futures::stream::unfold(state, |mut state| async move {
        let mut packet_buf: Vec<u8> = Vec::new();
        let mut packet_len: usize = 0;
        let mut end_of_stream = false;
        loop {
            if !end_of_stream {
                match state.body.data().await.transpose() {
                    Ok(Some(data)) => state.buffer.push(data),
                    Ok(None) => end_of_stream = true,
                    Err(_) => todo!(),
                }
            }

            let mut reader = (&mut state.buffer).reader();
            if packet_len == 0 {
                loop {
                    let (done, used) = {
                        let available = match reader.fill_buf() {
                            Ok(n) => n,
                            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                            Err(e) => return Some((Err(Error::Io(e)), state)),
                        };
                        match memchr::memchr(PACKET_SEPARATOR_V3, available) {
                            Some(i) => {
                                packet_len = match std::str::from_utf8(&available[..i])
                                    .map_err(|_| Error::InvalidPacketLength)
                                    .and_then(|s| {
                                        s.parse::<usize>().map_err(|_| Error::InvalidPacketLength)
                                    }) {
                                    Ok(size) => size,
                                    Err(_) => {
                                        return Some((Err(Error::InvalidPacketLength), state))
                                    }
                                };
                                (true, i + 1)
                            }
                            None if end_of_stream => return None,
                            None => (false, 0),
                        }
                    };
                    reader.consume(used);
                    if done || used == 0 {
                        break;
                    }
                }
            }

            if packet_len == 0 {
                continue;
            }

            let byte_read = {
                let data = reader.fill_buf().unwrap();
                match std::str::from_utf8(data) {
                    Ok(chunk) => {
                        let i = chunk.char_indices().nth(packet_len).map(|(i, _)| i);
                        if let Some(i) = i {
                            packet_buf.extend_from_slice(&chunk[..i].as_bytes());
                            i
                        } else {
                            packet_buf.extend_from_slice(chunk.as_bytes());
                            chunk.len()
                        }
                    }
                    Err(e) => {
                        let chunk =
                            unsafe { std::str::from_utf8_unchecked(&data[..e.valid_up_to()]) };

                        let i = chunk.char_indices().nth(packet_len).map(|(i, _)| i);
                        if let Some(i) = i {
                            packet_buf.extend_from_slice(&chunk[..i].as_bytes());
                            i
                        } else {
                            packet_buf.extend_from_slice(data);
                            data.len()
                        }
                    }
                }
            };
            reader.consume(byte_read);

            let packet_str = std::str::from_utf8(&packet_buf);
            if packet_str
                .map(|p| p.chars().count() == packet_len)
                .unwrap_or_default()
            {
                break Some((
                    Packet::try_from(unsafe { std::str::from_utf8_unchecked(&packet_buf) })
                        .map_err(|e| Error::InvalidPacketLength),
                    state,
                ));
            } else if end_of_stream && state.buffer.remaining() == 0 {
                break None;
            }
        }
    })
}
#[cfg(all(feature = "v3", feature = "v4"))]
pub fn body_parser(
    body: impl http_body::Body + Unpin,
    #[allow(unused_variables)] protocol: ProtocolVersion,
) -> impl Stream<Item = Result<Packet, Error>> {
    #[cfg(all(feature = "v3", feature = "v4"))]
    return match protocol {
        ProtocolVersion::V4 => body_parser_v4(body),
        ProtocolVersion::V3 => body_parser_v3(body),
    };

    #[cfg(feature = "v4")]
    return body_parser_v4(body);

    #[cfg(feature = "v3")]
    return body_parser_v3(body);
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
    async fn test_payload_v4() {
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
        let stream = hyper::Body::wrap_stream(futures::stream::iter(
            DATA.chunks(2).map(Ok::<_, std::convert::Infallible>),
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

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn test_payload_iterator_v3() {
        assert!(cfg!(feature = "v3"));

        let data = Full::new(Bytes::from("4:4foo3:4€f2:4f"));
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
            Packet::Message(msg) if msg == "f"
        ));
        assert_eq!(payload.next().await.is_none(), true);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn test_payload_stream_v3() {
        assert!(cfg!(feature = "v3"));
        const DATA: &[u8] = "4:4foo3:4€f2:4fo".as_bytes();
        let stream = hyper::Body::wrap_stream(futures::stream::iter(
            DATA.chunks(2).map(Ok::<_, std::convert::Infallible>),
        ));
        let payload = body_parser_v3(stream);
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
