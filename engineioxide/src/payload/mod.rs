use futures::Stream;

use crate::{errors::Error, packet::Packet, service::ProtocolVersion};
use bytes::Buf;
use std::io::BufRead;

use self::buf::BufList;

mod buf;
pub const PACKET_SEPARATOR_V4: u8 = b'\x1e';
pub const PACKET_SEPARATOR_V3: u8 = b':';

#[cfg(feature = "v4")]
fn body_parser_v4(body: impl http_body::Body + Unpin) -> impl Stream<Item = Result<Packet, Error>> {
    struct InnerPayload<B: http_body::Body> {
        body: B,
        buffer: BufList<B::Data>,
    }
    let state = InnerPayload {
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
}

pub fn body_parser(
    body: impl http_body::Body + Unpin,
    #[allow(unused_variables)] protocol: ProtocolVersion,
) -> impl Stream<Item = Result<Packet, Error>> {
    #[cfg(feature = "v4")]
    return body_parser_v4(body);

    #[cfg(feature = "v3")]
    return body_parser_v3(body);

    #[cfg(all(feature = "v3", feature = "v4"))]
    match protocol {
        ProtocolVersion::V4 => body_parser_v4(body),
        ProtocolVersion::V3 => body_parser_v3(body),
    }
}

// impl<B: http_body::Body> Payload<B> {
//     #[cfg(feature = "v3")]
//     fn next_v3(&mut self) -> Poll<Option<Item>> {
//         use utf8_chars::BufReadCharsExt;
//         self.buffer.clear();
//         match self
//             .reader
//             .read_until(PACKET_SEPARATOR_V3, &mut self.buffer)
//         {
//             Ok(bytes_read) => (bytes_read > 0).then(|| {
//                 if self.buffer.ends_with(&[PACKET_SEPARATOR_V3]) {
//                     self.buffer.pop();
//                 }
//                 let char_len = std::str::from_utf8(&self.buffer)
//                     .map_err(|_| Error::InvalidPacketLength)
//                     .and_then(|s| s.parse::<usize>().map_err(|_| Error::InvalidPacketLength))?;

//                 let mut cursor = 0;
//                 self.buffer.clear();
//                 self.buffer.resize(char_len * 4, 0);
//                 for char in self.reader.chars().take(char_len) {
//                     let char = char?;
//                     char.encode_utf8(&mut self.buffer[cursor..]);
//                     cursor += char.len_utf8();
//                 }

//                 // There is no need to recheck the buffer length here, since it is already checked with the chars() iterator
//                 let buffer_ref = unsafe { std::str::from_utf8_unchecked(&self.buffer[..cursor]) };
//                 Packet::try_from(buffer_ref)
//             }),
//             Err(e) => Some(Err(Error::Io(e))),
//         }
//     }
// }

#[cfg(test)]
mod tests {

    use std::convert::Infallible;

    use bytes::Bytes;
    use futures::StreamExt;
    use http_body::Full;
    use hyper::Body;

    use crate::{packet::Packet, payload::body_parser_v4};
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

    #[tokio::test]
    async fn test_payload_stream_v4() {
        assert!(cfg!(feature = "v4"));
        const DATA: &[u8] = "4foo\x1e4€f\x1e4fo".as_bytes();
        let stream = Body::wrap_stream(futures::stream::iter(
            DATA.chunks(2).map(Ok::<_, Infallible>),
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

    // #[tokio::test]
    // async fn test_payload_iterator_v3() {
    //     assert!(cfg!(feature = "v3"));

    //     let data = Full::new(Bytes::from("4:4foo3:4€f2:4f"));
    //     let payload = Payload::new(ProtocolVersion::V3, data);
    //     futures::pin_mut!(payload);
    //     assert!(matches!(
    //         payload.next().await.unwrap().unwrap(),
    //         Packet::Message(msg) if msg == "foo"
    //     ));
    //     assert!(matches!(
    //         payload.next().await.unwrap().unwrap(),
    //         Packet::Message(msg) if msg == "€f"
    //     ));
    //     assert!(matches!(
    //         payload.next().await.unwrap().unwrap(),
    //         Packet::Message(msg) if msg == "f"
    //     ));
    //     assert_eq!(payload.next().await.is_none(), true);
    // }
}
