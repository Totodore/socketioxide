use std::borrow::Cow;
use std::sync::Arc;
use serde::de::DeserializeOwned;
use serde_json::Value;
use crate::client::SocketData;
use crate::errors::Error;
use crate::packet::{BinaryPacket, Packet, PacketData};
use crate::packet::PacketData::{BinaryAck, BinaryEvent, Connect, ConnectError, Disconnect, Event, EventAck};
use crate::parser::{Emittable, Parser};
use engineioxide::Socket as EIoSocket;

/// The default packet parsing implementation for Socket.io following.
/// Protocol reference can be looked up [here](https://github.com/socketio/socket.io-protocol?tab=readme-ov-file).
#[derive(Clone, Default)]
pub struct DefaultParser;

impl DefaultParser {
    /// Get the max size the packet could have when serialized
    /// This is used to pre-allocate a buffer for the packet
    ///
    /// #### Disclaimer: The size does not include serialized `Value` size
    pub(crate) fn get_size_hint(packet: &Packet) -> usize {
        use crate::packet::PacketData::*;
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

    fn index(packet: &PacketData) -> char {
        match packet {
            PacketData::Connect(_) => '0',
            PacketData::Disconnect => '1',
            PacketData::Event(_, _, _) => '2',
            PacketData::EventAck(_, _) => '3',
            PacketData::ConnectError => '4',
            PacketData::BinaryEvent(_, _, _) => '5',
            PacketData::BinaryAck(_, _) => '6',
        }
    }


    /// Deserialize an event packet from a string, formated as:
    /// ```text
    /// ["<event name>", ...<JSON-stringified payload without binary>]
    /// ```
    fn deserialize_event_packet(data: &str) -> Result<(String, Value), Error> {
        #[cfg(feature = "tracing")]
        tracing::debug!("Deserializing event packet: {:?}", data);
        let packet = match serde_json::from_str::<Value>(data)? {
            Value::Array(packet) => packet,
            _ => return Err(Error::InvalidEventName),
        };

        let event = packet
            .first()
            .ok_or(Error::InvalidEventName)?
            .as_str()
            .ok_or(Error::InvalidEventName)?
            .to_string();
        let payload = Value::from_iter(packet.into_iter().skip(1));
        Ok((event, payload))
    }

    fn deserialize_packet<T: DeserializeOwned>(data: &str) -> Result<Option<T>, serde_json::Error> {
        #[cfg(feature = "tracing")]
        tracing::debug!("Deserializing packet: {:?}", data);
        let packet = if data.is_empty() {
            None
        } else {
            Some(serde_json::from_str(data)?)
        };
        Ok(packet)
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

        let capacity = Self::get_size_hint(&packet) + data.as_ref().map(|d| d.len()).unwrap_or(0);
        let mut res = String::with_capacity(capacity);
        res.push(Self::index(&packet.inner));

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

    fn decode_msg<'a>(&self, msg: String, socket: Arc<EIoSocket<SocketData>>) -> Result<Packet<'a>, Error> {
        // It is possible to parse the packet from a byte slice because separators are only ASCII
        let chars = msg.as_bytes();
        let mut i = 1;
        let index = (b'0'..=b'6')
            .contains(&chars[0])
            .then_some(chars[0])
            .ok_or(Error::InvalidPacketType)?;

        // Move the cursor to skip the payload count if it is a binary packet
        if index == b'5' || index == b'6' {
            while chars.get(i) != Some(&b'-') {
                i += 1;
            }
            i += 1;
        }

        let start_index = i;
        // Custom nsps will start with a slash
        let ns = if chars.get(i) == Some(&b'/') {
            loop {
                match chars.get(i) {
                    Some(b',') => {
                        i += 1;
                        break Cow::Owned(msg[start_index..i - 1].to_string());
                    }
                    // It maybe possible depending on clients that ns does not end with a comma
                    // if it is the end of the packet
                    // e.g `1/custom`
                    None => {
                        break Cow::Owned(msg[start_index..i].to_string());
                    }
                    Some(_) => i += 1,
                }
            }
        } else {
            Cow::Borrowed("/")
        };

        let start_index = i;
        let ack: Option<i64> = loop {
            match chars.get(i) {
                Some(c) if c.is_ascii_digit() => i += 1,
                Some(b'[' | b'{') if i > start_index => break msg[start_index..i].parse().ok(),
                _ => break None,
            }
        };

        let data = &msg[i..];
        let inner = match index {
            b'0' => PacketData::Connect((!data.is_empty()).then(|| data.to_string())),
            b'1' => PacketData::Disconnect,
            b'2' => {
                let (event, payload) = Self::deserialize_event_packet(data)?;
                PacketData::Event(event.into(), payload, ack)
            }
            b'3' => {
                let packet = Self::deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::EventAck(packet, ack.ok_or(Error::InvalidPacketType)?)
            }
            b'5' => {
                let (event, payload) = Self::deserialize_event_packet(data)?;
                PacketData::BinaryEvent(event.into(), BinaryPacket::incoming(payload), ack)
            }
            b'6' => {
                let packet = Self::deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::BinaryAck(
                    BinaryPacket::incoming(packet),
                    ack.ok_or(Error::InvalidPacketType)?,
                )
            }
            _ => return Err(Error::InvalidPacketType),
        };

        Ok(Packet { inner, ns })
    }

    fn decode_bin<'a>(&self, bin: Vec<u8>, socket: Arc<EIoSocket<SocketData>>) -> Option<Packet<'a>> {
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid={}] applying payload on packet", socket.id);
        let is_complete = if let Some(ref mut packet) = *socket.data.partial_bin_packet.lock().unwrap() {
            match packet.inner {
                PacketData::BinaryEvent(_, ref mut bin_packet, _) | PacketData::BinaryAck(ref mut bin_packet, _) => {
                    bin_packet.add_payload(bin);
                    bin_packet.is_complete()
                }
                _ => unreachable!("partial_bin_packet should only be set for binary packets"),
            }
        } else {
            #[cfg(feature = "tracing")]
            tracing::debug!("[sid={}] socket received unexpected bin data", socket.id);
            false
        };

        if is_complete {
            return socket.data.partial_bin_packet.lock().unwrap().take();
        }
        None
    }
}