use std::{io::BufRead, vec};
use cfg_if::cfg_if;

use crate::service::ProtocolVersion;

pub const PACKET_SEPARATOR: u8 = b'\x1e';

/// A payload is a series of encoded packets tied together.
/// How packets are tied together depends on the protocol.
pub struct Payload<R: BufRead> {
    reader: R,
    buffer: Vec<u8>,
    #[allow(dead_code)]
    protocol: ProtocolVersion,
}

type Item = Result<String, String>;

impl<R: BufRead> Payload<R> {
    pub fn new(protocol: ProtocolVersion, data: R) -> Self {
        Payload {
            reader: data,
            buffer: vec![],
            protocol,
        }
    }

    #[cfg(feature = "v3")]
    fn next_v3(&mut self) -> Option<Item> {
        self.buffer.clear();

        match self.reader.read_until(b':', &mut self.buffer) {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    // remove trailing separator
                    if self.buffer.ends_with(&[b':']) {
                        self.buffer.pop();
                    }

                    let length = match String::from_utf8(self.buffer.clone()) {
                        Ok(s) => {
                            if let Ok(l) = s.parse::<usize>() {
                                l
                            } else {
                                return Some(Err("Invalid packet length".into()));
                            }
                        },
                        Err(_) => return Some(Err("Invalid packet length".into())),
                    };

                    self.buffer.clear();
                    self.buffer.resize(length, 0);

                    match self.reader.read_exact(&mut self.buffer) {
                        Ok(_) => {
                            match String::from_utf8(self.buffer.clone()) {
                                Ok(s) => Some(Ok(s)),
                                Err(_) => Some(Err("Invalid packet data".into())),
                            }
                        },
                        Err(err) => Some(Err(err.to_string())),
                    }
                } else {
                    None
                }
            }
            Err(err) => Some(Err(err.to_string())),
        }
    }

    #[cfg(feature = "v4")]
    fn next_v4(&mut self) -> Option<Item> {
        self.buffer.clear();

        match self.reader.read_until(PACKET_SEPARATOR, &mut self.buffer) {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    // remove trailing separator
                    if self.buffer.ends_with(&[PACKET_SEPARATOR]) {
                        self.buffer.pop();
                    }
                    
                    match String::from_utf8( self.buffer.clone()) {
                        Ok(s) => Some(Ok(s)),
                        Err(_) => Some(Err("Packet is not a valid UTF-8 string".into())),
                    }
                } else {
                    None
                }
            }
            Err(err) => Some(Err(err.to_string())),
        }
    }
}

impl<R: BufRead> Iterator for Payload<R> {
    type Item = Item;

    cfg_if! {
        if #[cfg(all(feature = "v3", feature = "v4"))] {
            fn next(&mut self) -> Option<Self::Item> {
                match self.protocol {
                    ProtocolVersion::V3 => {
                        self.next_v3()
                    },
                    ProtocolVersion::V4 => {
                        self.next_v4()
                    },
                }
            }
        } else if #[cfg(feature = "v3")] {
            fn next(&mut self) -> Option<Self::Item> {
                self.next_v3()
            }
        } else {
            fn next(&mut self) -> Option<Self::Item> {
                self.next_v4()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::{BufReader, Cursor}, vec};

    use crate::service::ProtocolVersion;

    use super::{Payload, PACKET_SEPARATOR};

    #[test]
    fn test_payload_iterator_v4() -> Result<(), String> {
        assert!(cfg!(feature = "v4"));

        let data = BufReader::new(Cursor::new(vec![
            b'f', b'o', b'o', PACKET_SEPARATOR, b'f', b'o', PACKET_SEPARATOR, b'f',
        ]));
        let mut payload = Payload::new(ProtocolVersion::V4, data);

        assert_eq!(payload.next(), Some(Ok("foo".into())));
        assert_eq!(payload.next(), Some(Ok("fo".into())));
        assert_eq!(payload.next(), Some(Ok("f".into())));
        assert_eq!(payload.next(), None);

        Ok(())
    }

    #[test]
    fn test_payload_iterator_v3() -> Result<(), String> {
        assert!(cfg!(feature = "v3"));

        let data = BufReader::new(Cursor::new(vec![
            b'3', b':', b'f', b'o', b'o', b'2', b':', b'f', b'o', b'1', b':', b'f',
        ]));
        let mut payload = Payload::new(ProtocolVersion::V3, data);

        assert_eq!(payload.next(), Some(Ok("foo".into())));
        assert_eq!(payload.next(), Some(Ok("fo".into())));
        assert_eq!(payload.next(), Some(Ok("f".into())));
        assert_eq!(payload.next(), None);

        Ok(())
    }
}
