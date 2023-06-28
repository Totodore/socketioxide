use std::{io::BufRead, vec};

use crate::protocol::ProtocolVersion;

const PACKET_SEPARATOR: u8 = b'\x1e';

/// A payload is a series of encoded packets tied together.
/// How packets are tied together depends on the protocol.
pub struct Payload<R: BufRead> {
    reader: R,
    buffer: Vec<u8>,
    protocol: ProtocolVersion,
}

impl<R: BufRead> Payload<R> {
    pub fn new(data: R, protocol: ProtocolVersion) -> Self {
        Payload {
            reader: data,
            buffer: vec![],
            protocol,
        }
    }
}

impl<R: BufRead> Iterator for Payload<R> {
    type Item = Result<String, String>;

    fn next(&mut self) -> Option<Self::Item> {
        self.buffer.clear();

        match self.protocol {
            ProtocolVersion::V3 => {
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
            },
            ProtocolVersion::V4 => {
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::{BufReader, Cursor}, vec};

    use crate::protocol::ProtocolVersion;

    use super::{Payload, PACKET_SEPARATOR};

    #[test]
    fn test_payload_iterator_v4() -> Result<(), String> {
        let data = BufReader::new(Cursor::new(vec![
            b'f', b'o', b'o', PACKET_SEPARATOR, b'f', b'o', PACKET_SEPARATOR, b'f',
        ]));
        let mut payload = Payload::new(data, ProtocolVersion::V4);

        assert_eq!(payload.next(), Some(Ok("foo".into())));
        assert_eq!(payload.next(), Some(Ok("fo".into())));
        assert_eq!(payload.next(), Some(Ok("f".into())));
        assert_eq!(payload.next(), None);

        Ok(())
    }

    #[test]
    fn test_payload_iterator_v3() -> Result<(), String> {
        let data = BufReader::new(Cursor::new(vec![
            b'3', b':', b'f', b'o', b'o', b'2', b':', b'f', b'o', b'1', b':', b'f',
        ]));
        let mut payload = Payload::new(data, ProtocolVersion::V3);

        assert_eq!(payload.next(), Some(Ok("foo".into())));
        assert_eq!(payload.next(), Some(Ok("fo".into())));
        assert_eq!(payload.next(), Some(Ok("f".into())));
        assert_eq!(payload.next(), None);

        Ok(())
    }
}
