use std::{io::BufRead, vec};

use crate::{errors::Error, packet::Packet, service::ProtocolVersion};
use utf8_chars::BufReadCharsExt;

pub const PACKET_SEPARATOR_V4: u8 = b'\x1e';
pub const PACKET_SEPARATOR_V3: u8 = b':';

pub struct Payload<R: BufRead> {
    buffer: Vec<u8>,
    reader: R,
    #[allow(dead_code)]
    protocol: ProtocolVersion,
}

type Item = Result<Packet, Error>;

impl<R: BufRead> Payload<R> {
    pub fn new(protocol: ProtocolVersion, reader: R) -> Self {
        Self {
            buffer: vec![],
            reader,
            protocol,
        }
    }
}

impl<R: BufRead> Payload<R> {
    #[cfg(feature = "v3")]
    fn next_v3(&mut self) -> Option<Item> {
        self.buffer.clear();
        match self
            .reader
            .read_until(PACKET_SEPARATOR_V3, &mut self.buffer)
        {
            Ok(bytes_read) => (bytes_read > 0).then(|| {
                if self.buffer.ends_with(&[PACKET_SEPARATOR_V3]) {
                    self.buffer.pop();
                }
                let char_len = std::str::from_utf8(&self.buffer)
                    .map_err(|_| Error::InvalidPacketLength)
                    .and_then(|s| s.parse::<usize>().map_err(|_| Error::InvalidPacketLength))?;

                let mut cursor = 0;
                self.buffer.clear();
                self.buffer.resize(char_len * 4, 0);
                for char in self.reader.chars().take(char_len) {
                    let char = char?;
                    char.encode_utf8(&mut self.buffer[cursor..]);
                    cursor += char.len_utf8();
                }

                // There is no need to recheck the buffer length here, since it is already checked with the chars() iterator
                let buffer_ref = unsafe { std::str::from_utf8_unchecked(&self.buffer[..cursor]) };
                Packet::try_from(buffer_ref)
            }),
            Err(e) => Some(Err(Error::Io(e))),
        }
    }

    #[cfg(feature = "v4")]
    fn next_v4(&mut self) -> Option<Item> {
        self.buffer.clear();
        match self
            .reader
            .read_until(PACKET_SEPARATOR_V4, &mut self.buffer)
        {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    let buffer_ref: &[u8] = if self.buffer.ends_with(&[PACKET_SEPARATOR_V4]) {
                        &self.buffer[..self.buffer.len() - 1]
                    } else {
                        &self.buffer[..]
                    };

                    match std::str::from_utf8(buffer_ref) {
                        Ok(packet) => Some(Packet::try_from(packet)),
                        Err(e) => Some(Err(e.into())),
                    }
                } else {
                    None
                }
            }
            Err(e) => Some(Err(Error::Io(e))),
        }
    }
}

impl<R: BufRead> Iterator for Payload<R> {
    type Item = Item;

    #[cfg(all(feature = "v3", feature = "v4"))]
    fn next(&mut self) -> Option<Self::Item> {
        match self.protocol {
            ProtocolVersion::V3 => self.next_v3(),
            ProtocolVersion::V4 => self.next_v4(),
        }
    }

    #[cfg(feature = "v3")]
    #[cfg(not(feature = "v4"))]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_v3()
    }

    #[cfg(feature = "v4")]
    #[cfg(not(feature = "v3"))]
    fn next(&mut self) -> Option<Self::Item> {
        self.next_v4()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufReader, Cursor},
        vec,
    };

    use crate::{packet::Packet, payload::Payload, service::ProtocolVersion};

    use super::PACKET_SEPARATOR_V3;
    use super::PACKET_SEPARATOR_V4;

    #[test]
    fn test_payload_iterator_v4() {
        assert!(cfg!(feature = "v4"));

        let data = BufReader::new(Cursor::new(vec![
            b'4',
            b'f',
            b'o',
            b'o',
            PACKET_SEPARATOR_V4,
            b'4',
            0xe2,
            0x82,
            0xac, // € on three bytes
            b'f',
            PACKET_SEPARATOR_V4,
            b'4',
            b'f',
        ]));
        let mut payload = Payload::new(ProtocolVersion::V4, data);

        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "foo"
        ));
        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "€f"
        ));
        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "f"
        ));
        assert_eq!(payload.next().is_none(), true);
    }

    #[test]
    fn test_payload_iterator_v3() -> Result<(), String> {
        assert!(cfg!(feature = "v3"));

        let data = BufReader::new(Cursor::new(vec![
            // First packet
            b'4',
            PACKET_SEPARATOR_V3,
            b'4',
            b'f',
            b'o',
            b'o',
            // Second packet
            b'3',
            PACKET_SEPARATOR_V3,
            b'4',
            0xe2,
            0x82,
            0xac, // € on three bytes
            b'f',
            // Third packet
            b'2',
            PACKET_SEPARATOR_V3,
            b'4',
            b'f',
        ]));
        let mut payload = Payload::new(ProtocolVersion::V3, data);
        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "foo"
        ));
        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "€f"
        ));
        assert!(matches!(
            payload.next().unwrap().unwrap(),
            Packet::Message(msg) if msg == "f"
        ));
        assert_eq!(payload.next().is_none(), true);

        Ok(())
    }
}
