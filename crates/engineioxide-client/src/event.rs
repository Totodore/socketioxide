use bytes::Bytes;
use engineioxide_core::{Packet, Str, TransportType};

#[derive(Debug, PartialEq)]
pub enum EioEvent {
    Connect,
    Disconnect,
    Message(Str),
    Binary(Bytes),
    Upgrade(TransportType),
}

impl From<EioEvent> for Option<Packet> {
    fn from(value: EioEvent) -> Option<Packet> {
        match value {
            EioEvent::Message(msg) => Some(Packet::Message(msg)),
            EioEvent::Binary(bin) => Some(Packet::Binary(bin)),
            _ => None,
        }
    }
}
