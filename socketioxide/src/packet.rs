//! Socket.io packet implementation.
//! The [`Packet`] is the base unit of data that is sent over the engine.io socket.
//! It should not be used directly except when implementing the [`Adapter`](crate::adapter::Adapter) trait.
use std::borrow::Cow;

use crate::{payload_value::PayloadValue, ProtocolVersion};
use bytes::Bytes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::errors::Error;
use engineioxide::sid::Sid;

/// The socket.io packet type.
/// Each packet has a type and a namespace
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet<'a> {
    /// The packet data
    pub inner: PacketData<'a>,
    /// The namespace the packet belongs to
    pub ns: Cow<'a, str>,
}

impl<'a> Packet<'a> {
    /// Send a connect packet with a default payload for v5 and no payload for v4
    pub fn connect(
        ns: &'a str,
        #[allow(unused_variables)] sid: Sid,
        #[allow(unused_variables)] protocol: ProtocolVersion,
    ) -> Self {
        #[cfg(not(feature = "v4"))]
        {
            Self::connect_v5(ns, sid)
        }

        #[cfg(feature = "v4")]
        {
            match protocol {
                ProtocolVersion::V4 => Self::connect_v4(ns),
                ProtocolVersion::V5 => Self::connect_v5(ns, sid),
            }
        }
    }

    /// Sends a connect packet without payload.
    #[cfg(feature = "v4")]
    fn connect_v4(ns: &'a str) -> Self {
        Self {
            inner: PacketData::Connect(None),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Sends a connect packet with payload.
    fn connect_v5(ns: &'a str, sid: Sid) -> Self {
        let val = serde_json::to_string(&ConnectPacket { sid }).unwrap();
        Self {
            inner: PacketData::Connect(Some(val)),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create a disconnect packet for the given namespace
    pub fn disconnect(ns: &'a str) -> Self {
        Self {
            inner: PacketData::Disconnect,
            ns: Cow::Borrowed(ns),
        }
    }
}

impl<'a> Packet<'a> {
    /// Create a connect error packet for the given namespace
    pub fn invalid_namespace(ns: &'a str) -> Self {
        Self {
            inner: PacketData::ConnectError,
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create an event packet for the given namespace
    pub fn event(
        ns: impl Into<Cow<'a, str>>,
        e: impl Into<Cow<'a, str>>,
        data: PayloadValue,
    ) -> Self {
        Self {
            inner: PacketData::Event(e.into(), data, None),
            ns: ns.into(),
        }
    }

    /// Create a binary event packet for the given namespace
    pub fn bin_event(
        ns: impl Into<Cow<'a, str>>,
        e: impl Into<Cow<'a, str>>,
        data: PayloadValue,
    ) -> Self {
        let packet = BinaryPacket::outgoing(data);
        Self {
            inner: PacketData::BinaryEvent(e.into(), packet, None),
            ns: ns.into(),
        }
    }

    /// Create a binary event packet for the given namespace, with binary payloads
    pub fn bin_event_with_payloads(
        ns: impl Into<Cow<'a, str>>,
        e: impl Into<Cow<'a, str>>,
        data: PayloadValue,
        bins: Vec<Bytes>,
    ) -> Self {
        let mut packet = BinaryPacket::outgoing(data);
        for bin in bins {
            packet.add_payload(bin);
        }
        Self {
            inner: PacketData::BinaryEvent(e.into(), packet, None),
            ns: ns.into(),
        }
    }

    /// Create an ack packet for the given namespace
    pub fn ack(ns: &'a str, data: PayloadValue, ack: i64) -> Self {
        Self {
            inner: PacketData::EventAck(data, ack),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create a binary ack packet for the given namespace
    pub fn bin_ack(ns: &'a str, data: PayloadValue, ack: i64) -> Self {
        let packet = BinaryPacket::outgoing(data);
        Self {
            inner: PacketData::BinaryAck(packet, ack),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Create a binary ack packet for the given namespace, with binary payloads
    pub fn bin_ack_with_payloads(
        ns: &'a str,
        data: PayloadValue,
        ack: i64,
        bins: Vec<Bytes>,
    ) -> Self {
        let mut packet = BinaryPacket::outgoing(data);
        for bin in bins {
            packet.add_payload(bin);
        }
        Self {
            inner: PacketData::BinaryAck(packet, ack),
            ns: Cow::Borrowed(ns),
        }
    }

    /// Get the max size the packet could have when serialized
    /// This is used to pre-allocate a buffer for the packet
    ///
    /// #### Disclaimer: The size does not include serialized `Value` size
    fn get_size_hint(&self) -> usize {
        use PacketData::*;
        const PACKET_INDEX_SIZE: usize = 1;
        const BINARY_PUNCTUATION_SIZE: usize = 2;
        const ACK_PUNCTUATION_SIZE: usize = 1;
        const NS_PUNCTUATION_SIZE: usize = 1;

        let data_size = match &self.inner {
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

        let nsp_size = if self.ns == "/" {
            0
        } else if self.ns.starts_with('/') {
            self.ns.len() + NS_PUNCTUATION_SIZE
        } else {
            self.ns.len() + NS_PUNCTUATION_SIZE + 1 // (1 for the leading slash)
        };
        data_size + nsp_size + PACKET_INDEX_SIZE
    }
}

/// | Type          | ID  | Usage                                                                                 |
/// |---------------|-----|---------------------------------------------------------------------------------------|
/// | CONNECT       | 0   | Used during the [connection to a namespace](#connection-to-a-namespace).              |
/// | DISCONNECT    | 1   | Used when [disconnecting from a namespace](#disconnection-from-a-namespace).          |
/// | EVENT         | 2   | Used to [send data](#sending-and-receiving-data) to the other side.                   |
/// | ACK           | 3   | Used to [acknowledge](#acknowledgement) an event.                                     |
/// | CONNECT_ERROR | 4   | Used during the [connection to a namespace](#connection-to-a-namespace).              |
/// | BINARY_EVENT  | 5   | Used to [send binary data](#sending-and-receiving-data) to the other side.            |
/// | BINARY_ACK    | 6   | Used to [acknowledge](#acknowledgement) an event (the response includes binary data). |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketData<'a> {
    /// Connect packet with optional payload (only used with v5 for response)
    Connect(Option<String>),
    /// Disconnect packet, used to disconnect from a namespace
    Disconnect,
    /// Event packet with optional ack id, to request an ack from the other side
    Event(Cow<'a, str>, PayloadValue, Option<i64>),
    /// Event ack packet, to acknowledge an event
    EventAck(PayloadValue, i64),
    /// Connect error packet, sent when the namespace is invalid
    ConnectError,
    /// Binary event packet with optional ack id, to request an ack from the other side
    BinaryEvent(Cow<'a, str>, BinaryPacket, Option<i64>),
    /// Binary ack packet, to acknowledge an event with binary data
    BinaryAck(BinaryPacket, i64),
}

/// Binary packet used when sending binary data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPacket {
    /// Data related to the packet
    pub(crate) data: PayloadValue,
    /// The number of expected payloads (used when receiving data)
    payload_count: usize,
    /// A place to receive binary payloads until the packet is complete
    payloads: Vec<Bytes>,
}

impl<'a> PacketData<'a> {
    fn index(&self) -> char {
        match self {
            PacketData::Connect(_) => '0',
            PacketData::Disconnect => '1',
            PacketData::Event(_, _, _) => '2',
            PacketData::EventAck(_, _) => '3',
            PacketData::ConnectError => '4',
            PacketData::BinaryEvent(_, _, _) => '5',
            PacketData::BinaryAck(_, _) => '6',
        }
    }

    /// Set the ack id for the packet
    /// It will only set the ack id for the packets that support it
    pub(crate) fn set_ack_id(&mut self, ack_id: i64) {
        match self {
            PacketData::Event(_, _, ack) | PacketData::BinaryEvent(_, _, ack) => {
                *ack = Some(ack_id)
            }
            _ => {}
        };
    }

    /// Check if the packet is a binary packet (either binary event or binary ack)
    pub(crate) fn is_binary(&self) -> bool {
        matches!(
            self,
            PacketData::BinaryEvent(_, _, _) | PacketData::BinaryAck(_, _)
        )
    }

    /// Get the number of binary payloads or 0 if it is not a binary packet
    pub(crate) fn payload_count(&self) -> usize {
        match self {
            PacketData::BinaryEvent(_, bin, _) | PacketData::BinaryAck(bin, _) => bin.payload_count,
            _ => 0,
        }
    }
}

impl BinaryPacket {
    /// Create a binary packet from incoming data
    fn incoming(data: PayloadValue, payload_count: usize) -> Self {
        let actual_payload_count = data.count_payloads();
        if payload_count != actual_payload_count {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                "Binary packet header claimed {} payloads, but found {} placeholders",
                payload_count,
                actual_payload_count
            );
        }

        Self {
            data,
            payload_count: actual_payload_count,
            payloads: vec![],
        }
    }

    /// Create a binary packet from outgoing data
    fn outgoing(data: PayloadValue) -> Self {
        let data = match data {
            arr @ PayloadValue::Array(_) => arr,
            d => PayloadValue::Array(vec![d]),
        };
        let payload_count = data.count_payloads();

        Self {
            data,
            payload_count,
            payloads: vec![],
        }
    }

    /// Add a payload to the binary packet, when all payloads are added,
    /// the packet is complete and can be further processed
    pub fn add_payload(&mut self, payload: Bytes) {
        if self.is_complete() {
            #[cfg(feature = "tracing")]
            tracing::warn!("Attempt to add payload to already-complete binary packet");
        } else {
            self.payloads.push(payload);

            if self.is_complete() {
                integrate_payloads(&mut self.data, &mut self.payloads);
            }
        }
    }

    /// Check if the binary packet is complete, it means that all payloads have been received
    pub fn is_complete(&self) -> bool {
        self.payload_count == self.payloads.len()
    }

    pub(crate) fn extract_payloads(&mut self) -> Vec<Bytes> {
        self.data.get_binary_payloads()
    }
}

fn integrate_payloads(data: &mut PayloadValue, payloads: &mut Vec<Bytes>) {
    match data {
        PayloadValue::Binary(n, ref mut data) => {
            if let Some(payload) = payloads.get_mut(*n) {
                std::mem::swap(payload, data);
            } else {
                #[cfg(feature = "tracing")]
                tracing::warn!(
                    "Binary packet structure included placeholder with num out of range"
                );
            }
        }
        PayloadValue::Object(o) => {
            for value in o.values_mut() {
                integrate_payloads(value, payloads);
            }
        }
        PayloadValue::Array(a) => {
            for value in a.iter_mut() {
                integrate_payloads(value, payloads);
            }
        }
        _ => (),
    }
}

impl<'a> From<Packet<'a>> for String {
    fn from(mut packet: Packet<'a>) -> String {
        use PacketData::*;

        // Serialize the data if there is any
        // pre-serializing allows to preallocate the buffer
        let data = match &mut packet.inner {
            Event(e, data, _) | BinaryEvent(e, BinaryPacket { data, .. }, _) => {
                // Expand the packet if it is an array with data -> ["event", ...data]
                let packet = match data {
                    PayloadValue::Array(ref mut v) if !v.is_empty() => {
                        v.insert(0, PayloadValue::String((*e).to_string()));
                        data.to_json_string()
                    }
                    PayloadValue::Array(_) => serde_json::to_string::<(_, [(); 0])>(&(e, [])),
                    _ => serde_json::to_string(&(e, data.as_json())),
                }
                .unwrap();
                Some(packet)
            }
            EventAck(data, _) | BinaryAck(BinaryPacket { data, .. }, _) => {
                // Enforce that the packet is an array -> [data]
                let packet = match data {
                    PayloadValue::Array(_) => data.to_json_string(),
                    PayloadValue::Null => Ok("[]".to_string()),
                    _ => serde_json::to_string(&[data.as_json()]),
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

        res
    }
}

/// Deserialize an event packet from a string, formated as:
/// ```text
/// ["<event name>", ...<JSON-stringified payload without binary>]
/// ```
fn deserialize_event_packet(data: &str) -> Result<(String, PayloadValue), Error> {
    #[cfg(feature = "tracing")]
    tracing::debug!("Deserializing event packet: {:?}", data);
    let packet = match serde_json::from_str::<PayloadValue>(data)? {
        PayloadValue::Array(packet) => packet,
        _ => return Err(Error::InvalidEventName),
    };

    let event = packet
        .first()
        .ok_or(Error::InvalidEventName)?
        .as_str()
        .ok_or(Error::InvalidEventName)?
        .to_string();
    let payload = PayloadValue::from_iter(packet.into_iter().skip(1));
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

/// Deserialize a packet from a string
/// The string should be in the format of:
/// ```text
/// <packet type>[<# of binary attachments>-][<namespace>,][<acknowledgment id>][JSON-stringified payload without binary]
/// + binary attachments extracted
/// ```
impl<'a> TryFrom<String> for Packet<'a> {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // It is possible to parse the packet from a byte slice because separators are only ASCII
        let chars = value.as_bytes();
        let mut i = 1;
        let index = (b'0'..=b'6')
            .contains(&chars[0])
            .then_some(chars[0])
            .ok_or(Error::InvalidPacketType)?;

        // Move the cursor to skip the payload count if it is a binary packet
        let payload_count = if index == b'5' || index == b'6' {
            while chars.get(i) != Some(&b'-') {
                i += 1;
            }
            i += 1;

            std::str::from_utf8(&chars[1..(i - 1)])
                .map_err(|_| Error::InvalidPayloadCount)
                .and_then(|s| s.parse::<usize>().map_err(|_| Error::InvalidPayloadCount))?
        } else {
            0
        };

        let start_index = i;
        // Custom nsps will start with a slash
        let ns = if chars.get(i) == Some(&b'/') {
            loop {
                match chars.get(i) {
                    Some(b',') => {
                        i += 1;
                        break Cow::Owned(value[start_index..i - 1].to_string());
                    }
                    // It maybe possible depending on clients that ns does not end with a comma
                    // if it is the end of the packet
                    // e.g `1/custom`
                    None => {
                        break Cow::Owned(value[start_index..i].to_string());
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
                Some(b'[' | b'{') if i > start_index => break value[start_index..i].parse().ok(),
                _ => break None,
            }
        };

        let data = &value[i..];
        let inner = match index {
            b'0' => PacketData::Connect((!data.is_empty()).then(|| data.to_string())),
            b'1' => PacketData::Disconnect,
            b'2' => {
                let (event, payload) = deserialize_event_packet(data)?;
                PacketData::Event(event.into(), payload, ack)
            }
            b'3' => {
                let packet = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::EventAck(packet, ack.ok_or(Error::InvalidPacketType)?)
            }
            b'5' => {
                let (event, payload) = deserialize_event_packet(data)?;
                PacketData::BinaryEvent(
                    event.into(),
                    BinaryPacket::incoming(payload, payload_count),
                    ack,
                )
            }
            b'6' => {
                let packet = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::BinaryAck(
                    BinaryPacket::incoming(packet, payload_count),
                    ack.ok_or(Error::InvalidPacketType)?,
                )
            }
            _ => return Err(Error::InvalidPacketType),
        };

        Ok(Self { inner, ns })
    }
}

/// Connect packet sent by the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectPacket {
    sid: Sid,
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::*;

    fn packet_data_map(key: &'static str, value: &'static str) -> PayloadValue {
        PayloadValue::Object(
            [(key.to_string(), PayloadValue::String(value.to_string()))]
                .into_iter()
                .collect(),
        )
    }

    fn wrapped_packet_data_map(key: &'static str, value: &'static str) -> PayloadValue {
        PayloadValue::Array(vec![
            packet_data_map(key, value),
            PayloadValue::Binary(0, Bytes::from_static(&[1])),
        ])
    }

    #[test]
    fn packet_decode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(Packet::connect("/", sid, ProtocolVersion::V5), packet);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(Packet::connect("/admin™", sid, ProtocolVersion::V5), packet);
    }

    #[test]
    fn packet_encode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet: String = Packet::connect("/", sid, ProtocolVersion::V5)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet: String = Packet::connect("/admin™", sid, ProtocolVersion::V5)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);
    }

    // Disconnect,

    #[test]
    fn packet_decode_disconnect() {
        let payload = "1".to_string();
        let packet = Packet::try_from(payload).unwrap();
        assert_eq!(Packet::disconnect("/"), packet);

        let payload = "1/admin™,".to_string();
        let packet = Packet::try_from(payload).unwrap();
        assert_eq!(Packet::disconnect("/admin™"), packet);
    }

    #[test]
    fn packet_encode_disconnect() {
        let payload = "1".to_string();
        let packet: String = Packet::disconnect("/").try_into().unwrap();
        assert_eq!(packet, payload);

        let payload = "1/admin™,".to_string();
        let packet: String = Packet::disconnect("/admin™").try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // Event(String, Value, Option<i64>),
    #[test]
    fn packet_decode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value" }]));
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(
            Packet::event(
                "/",
                "event",
                PayloadValue::from_data(json!([{"data": "value"}])).unwrap()
            ),
            packet
        );

        // Check with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value" }]));
        let packet = Packet::try_from(payload).unwrap();

        let mut comparison_packet = Packet::event(
            "/",
            "event",
            PayloadValue::from_data(json!([{"data": "value"}])).unwrap(),
        );
        comparison_packet.inner.set_ack_id(1);
        assert_eq!(packet, comparison_packet);

        // Check with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(
            Packet::event(
                "/admin™",
                "event",
                PayloadValue::from_data(json!([{"data": "value™"}])).unwrap()
            ),
            packet
        );

        // Check with ack ID and NS
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::try_from(payload).unwrap();
        packet.inner.set_ack_id(1);

        let mut comparison_packet = Packet::event(
            "/admin™",
            "event",
            PayloadValue::from_data(json!([{"data": "value™"}])).unwrap(),
        );
        comparison_packet.inner.set_ack_id(1);

        assert_eq!(packet, comparison_packet);
    }

    #[test]
    fn packet_encode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value™" }]));
        let packet: String = Packet::event(
            "/",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);

        // Encode empty data
        let payload = format!("2{}", json!(["event", []]));
        let packet: String =
            Packet::event("/", "event", PayloadValue::from_data(json!([])).unwrap())
                .try_into()
                .unwrap();

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event(
            "/",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        );
        packet.inner.set_ack_id(1);
        let packet: String = packet.try_into().unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet: String = Packet::event(
            "/admin™",
            "event",
            PayloadValue::from_data(json!({"data": "value™"})).unwrap(),
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event(
            "/admin™",
            "event",
            PayloadValue::from_data(json!([{"data": "value™"}])).unwrap(),
        );
        packet.inner.set_ack_id(1);
        let packet: String = packet.try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // EventAck(Value, i64),
    #[test]
    fn packet_decode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(
            Packet::ack("/", PayloadValue::from_data(json!(["data"])).unwrap(), 54),
            packet
        );

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = Packet::try_from(payload).unwrap();

        assert_eq!(
            Packet::ack(
                "/admin™",
                PayloadValue::from_data(json!(["data"])).unwrap(),
                54
            ),
            packet
        );
    }

    #[test]
    fn packet_encode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet: String = Packet::ack("/", PayloadValue::from_data(json!("data")).unwrap(), 54)
            .try_into()
            .unwrap();
        assert_eq!(packet, payload);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet: String = Packet::ack(
            "/admin™",
            PayloadValue::from_data(json!("data")).unwrap(),
            54,
        )
        .try_into()
        .unwrap();
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_encode_connect_error() {
        let payload = format!("4{}", json!({ "message": "Invalid namespace" }));
        let packet: String = Packet::invalid_namespace("/").try_into().unwrap();
        assert_eq!(packet, payload);

        let payload = format!("4/admin™,{}", json!({ "message": "Invalid namespace" }));
        let packet: String = Packet::invalid_namespace("/admin™").try_into().unwrap();
        assert_eq!(packet, payload);
    }

    // BinaryEvent(String, BinaryPacket, Option<i64>),
    #[test]
    fn packet_encode_binary_event() {
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("51-{}", json);
        let packet: String = Packet::bin_event(
            "/",
            "event",
            PayloadValue::from_data(
                json!([{ "data": "value™" }, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = Packet::bin_event(
            "/",
            "event",
            PayloadValue::from_data(
                json!([{ "data": "value™" }, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
        );
        packet.inner.set_ack_id(254);
        let packet: String = packet.try_into().unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("51-/admin™,{}", json);
        let packet: String = Packet::bin_event(
            "/admin™",
            "event",
            PayloadValue::from_data(
                json!([{"data": "value™"}, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::bin_event(
            "/admin™",
            "event",
            PayloadValue::from_data(
                json!([{"data": "value™"}, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
        );
        packet.inner.set_ack_id(254);
        let packet: String = packet.try_into().unwrap();
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_event() {
        let binary_payload = Bytes::from_static(&[1]);

        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryEvent(
                "event".into(),
                BinaryPacket {
                    data: wrapped_packet_data_map("data", "value™"),
                    payload_count: 1,
                    payloads: vec![Bytes::new()],
                },
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("51-{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(None, "/"));

        // Check with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(Some(254), "/"));

        // Check with NS
        let payload = format!("51-/admin™,{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(None, "/admin™"));

        // Check with ack ID and NS
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryEvent(_, ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }
        assert_eq!(packet, comparison_packet(Some(254), "/admin™"));
    }

    // BinaryAck(BinaryPacket, i64),
    #[test]
    fn packet_encode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("61-54{}", json);
        let packet: String = Packet::bin_ack(
            "/",
            PayloadValue::from_data(
                json!([{ "data": "value™" }, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
            54,
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("61-/admin™,54{}", json);
        let packet: String = Packet::bin_ack(
            "/admin™",
            PayloadValue::from_data(
                json!([{ "data": "value™" }, { "_placeholder": true, "num": 0 }]),
            )
            .unwrap(),
            54,
        )
        .try_into()
        .unwrap();

        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_ack() {
        let binary_payload = Bytes::from_static(&[1]);

        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryAck(
                BinaryPacket {
                    data: wrapped_packet_data_map("data", "value™"),
                    payload_count: 1,
                    payloads: vec![Bytes::new()],
                },
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("61-54{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryAck(ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(54, "/"));

        // Check with NS
        let payload = format!("61-/admin™,54{}", json);
        let mut packet = Packet::try_from(payload).unwrap();
        match packet.inner {
            PacketData::BinaryAck(ref mut bin, _) => bin.add_payload(binary_payload.clone()),
            _ => (),
        }

        assert_eq!(packet, comparison_packet(54, "/admin™"));
    }

    #[test]
    fn packet_size_hint() {
        let sid = Sid::new();
        let len = serde_json::to_string(&ConnectPacket { sid }).unwrap().len();
        let packet = Packet::connect("/", sid, ProtocolVersion::V5);
        assert_eq!(packet.get_size_hint(), len + 1);

        let packet = Packet::connect("/admin", sid, ProtocolVersion::V5);
        assert_eq!(packet.get_size_hint(), len + 8);

        let packet = Packet::connect("admin", sid, ProtocolVersion::V4);
        assert_eq!(packet.get_size_hint(), 8);

        let packet = Packet::disconnect("/");
        assert_eq!(packet.get_size_hint(), 1);

        let packet = Packet::disconnect("/admin");
        assert_eq!(packet.get_size_hint(), 8);

        let packet = Packet::event(
            "/",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        );
        assert_eq!(packet.get_size_hint(), 1);

        let packet = Packet::event(
            "/admin",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        );
        assert_eq!(packet.get_size_hint(), 8);

        let packet = Packet::ack("/", PayloadValue::from_data(json!("data")).unwrap(), 54);
        assert_eq!(packet.get_size_hint(), 3);

        let packet = Packet::ack(
            "/admin",
            PayloadValue::from_data(json!("data")).unwrap(),
            54,
        );
        assert_eq!(packet.get_size_hint(), 10);

        let packet = Packet::bin_event(
            "/",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        );
        assert_eq!(packet.get_size_hint(), 3);

        let packet = Packet::bin_event(
            "/admin",
            "event",
            PayloadValue::from_data(json!({ "data": "value™" })).unwrap(),
        );
        assert_eq!(packet.get_size_hint(), 10);

        let packet = Packet::bin_ack("/", PayloadValue::from_data(json!("data")).unwrap(), 54);
        assert_eq!(packet.get_size_hint(), 5);
    }
}
