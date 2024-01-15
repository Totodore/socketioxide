use std::fmt::{Debug, Formatter};
use dyn_clone::DynClone;
use futures::AsyncReadExt;
use serde_json::Value;
use crate::packet::{BinaryPacket, Packet, PacketData};
use crate::packet::PacketData::{BinaryAck, BinaryEvent, ConnectError, Disconnect, Event, EventAck};

pub enum Emittable {
    String(String),
    Binary(Vec<u8>)
}

pub trait Parser: DynClone + Send + Sync + 'static {
    fn encode(&self, packet: Packet) -> Vec<Emittable>;
    fn decode_msg(&self, msg: String);
    fn decode_bin(&self, bin: Vec<u8>);
}

dyn_clone::clone_trait_object!(Parser);

impl Debug for dyn Parser {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parser()")
    }
}

#[derive(Clone, Default)]
pub struct DefaultParser;

impl DefaultParser {
    /// Get the max size the packet could have when serialized
    /// This is used to pre-allocate a buffer for the packet
    ///
    /// #### Disclaimer: The size does not include serialized `Value` size
    fn get_size_hint(packet: &Packet) -> usize {
        use PacketData::*;
        const PACKET_INDEX_SIZE: usize = 1;
        const BINARY_PUNCTUATION_SIZE: usize = 2;
        const ACK_PUNCTUATION_SIZE: usize = 1;
        const NS_PUNCTUATION_SIZE: usize = 1;

        let data_size = match &packet.inner {
            Connect(Some(data)) => data.len(),
            Connect(None) => 0,
            Disconnect => 0,
            Event(_, _, Some(ack)) => {
                ack.checked_ilog10().unwrap_or(0) as usize + ACK_PUNCTUATION_SIZE
            }
            Event(_, _, None) => 0,
            BinaryEvent(_, bin, None) => {
                bin.payload_count.checked_ilog10().unwrap_or(0) as usize + BINARY_PUNCTUATION_SIZE
            }
            BinaryEvent(_, bin, Some(ack)) => {
                ack.checked_ilog10().unwrap_or(0) as usize
                    + bin.payload_count.checked_ilog10().unwrap_or(0) as usize
                    + ACK_PUNCTUATION_SIZE
                    + BINARY_PUNCTUATION_SIZE
            }
            EventAck(_, ack) => ack.checked_ilog10().unwrap_or(0) as usize + ACK_PUNCTUATION_SIZE,
            BinaryAck(bin, ack) => {
                ack.checked_ilog10().unwrap_or(0) as usize
                    + bin.payload_count.checked_ilog10().unwrap_or(0) as usize
                    + ACK_PUNCTUATION_SIZE
                    + BINARY_PUNCTUATION_SIZE
            }
            ConnectError => 31,
        };

        let nsp_size = if packet.ns == "/" {
            0
        } else if packet.ns.starts_with('/') {
            packet.ns.len() + NS_PUNCTUATION_SIZE
        } else {
            packet.ns.len() + NS_PUNCTUATION_SIZE + 1 // (1 for the leading slash)
        };
        data_size + nsp_size + PACKET_INDEX_SIZE
    }
}

impl Parser for DefaultParser {
    fn encode(&self, mut packet: Packet) -> Vec<Emittable> {
        let bin_payloads = match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) | PacketData::BinaryAck(ref mut bin, _) => {
                std::mem::take(&mut bin.bin)
            }
            _ => vec![],
        };

        let bin_payloads = bin_payloads.iter()
            .map(|payload| Emittable::Binary(payload.clone()))
            .collect::<Vec<_>>();

        // Serialize the data if there is any
        // pre-serializing allows to preallocate the buffer
        let data = match &mut packet.inner {
            Event(e, data, _) | BinaryEvent(e, BinaryPacket { data, .. }, _) => {
                // Expand the packet if it is an array with data -> ["event", ...data]
                let packet = match data {
                    Value::Array(ref mut v) if !v.is_empty() => {
                        v.insert(0, Value::String((*e).to_string()));
                        serde_json::to_string(&v)
                    }
                    Value::Array(_) => serde_json::to_string::<(_, [(); 0])>(&(e, [])),
                    _ => serde_json::to_string(&(e, data)),
                }
                    .unwrap();
                Some(packet)
            }
            EventAck(data, _) | BinaryAck(BinaryPacket { data, .. }, _) => {
                // Enforce that the packet is an array -> [data]
                let packet = match data {
                    Value::Array(_) => serde_json::to_string(&data),
                    Value::Null => Ok("[]".to_string()),
                    _ => serde_json::to_string(&[data]),
                }
                    .unwrap();
                Some(packet)
            }
            _ => None,
        };

        let capacity = packet.get_size_hint() + data.as_ref().map(|d| d.len()).unwrap_or(0);
        let mut res = String::with_capacity(capacity);
        res.push(packet.inner.index());

        // Add the ns if it is not the default one and the packet is not binary
        // In case of bin packet, we should first add the payload count before ns
        let push_nsp = |res: &mut String| {
            if !packet.ns.is_empty() && packet.ns != "/" {
                if !packet.ns.starts_with('/') {
                    res.push('/');
                }
                res.push_str(&packet.ns);
                res.push(',');
            }
        };

        if !packet.inner.is_binary() {
            push_nsp(&mut res);
        }

        let mut itoa_buf = itoa::Buffer::new();

        match packet.inner {
            PacketData::Connect(Some(data)) => res.push_str(&data),
            PacketData::Disconnect | PacketData::Connect(None) => (),
            PacketData::Event(_, _, ack) => {
                if let Some(ack) = ack {
                    res.push_str(itoa_buf.format(ack));
                }

                res.push_str(&data.unwrap())
            }
            PacketData::EventAck(_, ack) => {
                res.push_str(itoa_buf.format(ack));
                res.push_str(&data.unwrap())
            }
            PacketData::ConnectError => res.push_str("{\"message\":\"Invalid namespace\"}"),
            PacketData::BinaryEvent(_, bin, ack) => {
                res.push_str(itoa_buf.format(bin.payload_count));
                res.push('-');

                push_nsp(&mut res);

                if let Some(ack) = ack {
                    res.push_str(itoa_buf.format(ack));
                }

                res.push_str(&data.unwrap())
            }
            PacketData::BinaryAck(packet, ack) => {
                res.push_str(itoa_buf.format(packet.payload_count));
                res.push('-');

                push_nsp(&mut res);

                res.push_str(itoa_buf.format(ack));
                res.push_str(&data.unwrap())
            }
        };

        return vec![Emittable::String(res)].into_iter()
            .chain(bin_payloads.into_iter())
            .collect();
    }

    fn decode_msg(&self, msg: String) {

    }

    fn decode_bin(&self, bin: Vec<u8>) {

    }
}