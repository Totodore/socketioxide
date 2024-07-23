use std::sync::Mutex;

use bytes::Bytes;
use engineioxide::Str;
use serde_json::Value;

use crate::{
    packet::{BinaryPacket, Packet, PacketData},
    parser::{Error, TransportPayload},
};

/// Parse and serialize from and into the socket.io common packet format.
///
/// The resulting string should be in the format of:
/// ```text
/// <packet type>[<# of binary attachments>-][<namespace>,][<acknowledgment id>][JSON-stringified payload without binary]
/// + binary attachments extracted
/// ```
#[derive(Debug, Default)]
pub struct CommonParser {
    /// Partial binary packet that is being received
    /// Stored here until all the binary payloads are received
    pub partial_bin_packet: Mutex<Option<Packet<'static>>>,
}

impl super::Parse for CommonParser {
    fn serialize<'a>(&self, mut packet: Packet<'a>) -> (TransportPayload, Vec<Bytes>) {
        use PacketData::*;

        // Serialize the data if there is any
        // pre-serializing allows to preallocate the buffer
        let data = match &mut packet.inner {
            Connect(Some(data)) => Some(serde_json::to_string(data).unwrap()),
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

        let capacity = get_size_hint(&packet) + data.as_ref().map(|d| d.len()).unwrap_or(0);
        let mut res = String::with_capacity(capacity);
        res.push(char::from_digit(packet.inner.index() as u32, 10).unwrap());

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

        match &packet.inner {
            PacketData::Connect(Some(_)) => res.push_str(&data.unwrap()),
            PacketData::Disconnect | PacketData::Connect(None) => (),
            PacketData::Event(_, _, ack) => {
                if let Some(ack) = *ack {
                    res.push_str(itoa_buf.format(ack));
                }

                res.push_str(&data.unwrap())
            }
            PacketData::EventAck(_, ack) => {
                res.push_str(itoa_buf.format(*ack));
                res.push_str(&data.unwrap())
            }
            PacketData::ConnectError(data) => res.push_str(data),
            PacketData::BinaryEvent(_, ref bin, ack) => {
                res.push_str(itoa_buf.format(bin.payload_count));
                res.push('-');

                push_nsp(&mut res);

                if let Some(ack) = *ack {
                    res.push_str(itoa_buf.format(ack));
                }

                res.push_str(&data.unwrap())
            }
            PacketData::BinaryAck(ref packet, ack) => {
                res.push_str(itoa_buf.format(packet.payload_count));
                res.push('-');

                push_nsp(&mut res);

                res.push_str(itoa_buf.format(*ack));
                res.push_str(&data.unwrap())
            }
        };

        let bins = match packet.inner {
            PacketData::BinaryEvent(_, bin, _) | PacketData::BinaryAck(bin, _) => {
                Vec::from_iter(bin.bin.into_iter().map(Bytes::from))
            }
            _ => Vec::new(),
        };
        (TransportPayload::Str(res.into()), bins)
    }

    fn parse_str(&self, value: Str) -> Result<Packet<'static>, Error> {
        let chars = value.as_bytes();
        // It is possible to parse the packet from a byte slice because separators are only ASCII
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
                        break value.slice(start_index..i - 1);
                    }
                    // It maybe possible depending on clients that ns does not end with a comma
                    // if it is the end of the packet
                    // e.g `1/custom`
                    None => {
                        break value.slice(start_index..i);
                    }
                    Some(_) => i += 1,
                }
            }
        } else {
            Str::from("/")
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
            b'0' => {
                if data.is_empty() {
                    PacketData::Connect(None)
                } else {
                    PacketData::Connect(serde_json::from_str(data)?)
                }
            }
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
                PacketData::BinaryEvent(event.into(), BinaryPacket::incoming(payload), ack)
            }
            b'6' => {
                let packet = deserialize_packet(data)?.ok_or(Error::InvalidPacketType)?;
                PacketData::BinaryAck(
                    BinaryPacket::incoming(packet),
                    ack.ok_or(Error::InvalidPacketType)?,
                )
            }
            _ => return Err(Error::InvalidPacketType),
        };

        if inner.is_binary() && !inner.is_complete() {
            *self.partial_bin_packet.lock().unwrap() = Some(Packet { inner, ns });
            Err(Error::NeedsMoreBinaryData)
        } else {
            Ok(Packet { inner, ns })
        }
    }

    fn parse_bin(&self, data: Bytes) -> Result<Packet<'static>, Error> {
        #[cfg(feature = "tracing")]
        tracing::debug!("[sid=] applying payload on packet"); // TODO: log sid
        let packet = &mut *self.partial_bin_packet.lock().unwrap();
        match packet {
            Some(Packet {
                inner:
                    PacketData::BinaryEvent(_, ref mut bin, _) | PacketData::BinaryAck(ref mut bin, _),
                ..
            }) => {
                bin.add_payload(data);
                if !bin.is_complete() {
                    Err(Error::NeedsMoreBinaryData)
                } else {
                    Ok(packet.take().unwrap())
                }
            }
            _ => Err(Error::UnexpectedBinaryPacket),
        }
    }
}

impl CommonParser {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Get the max size the packet could have when serialized
/// This is used to pre-allocate a buffer for the packet
///
/// #### Disclaimer: The size does not include serialized `Value` size
fn get_size_hint(packet: &Packet<'_>) -> usize {
    use PacketData::*;
    const PACKET_INDEX_SIZE: usize = 1;
    const BINARY_PUNCTUATION_SIZE: usize = 2;
    const ACK_PUNCTUATION_SIZE: usize = 1;
    const NS_PUNCTUATION_SIZE: usize = 1;

    let data_size = match &packet.inner {
        Connect(Some(data)) => 0,
        Connect(None) => 0,
        Disconnect => 0,
        Event(_, _, Some(ack)) => ack.checked_ilog10().unwrap_or(0) as usize + ACK_PUNCTUATION_SIZE,
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
        ConnectError(data) => data.len(),
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

fn deserialize_packet<T: serde::de::DeserializeOwned>(
    data: &str,
) -> Result<Option<T>, serde_json::Error> {
    #[cfg(feature = "tracing")]
    tracing::debug!("Deserializing packet: {:?}", data);
    let packet = if data.is_empty() {
        None
    } else {
        Some(serde_json::from_str(data)?)
    };
    Ok(packet)
}

#[cfg(test)]
mod test {
    use engineioxide::sid::Sid;
    use serde_json::json;

    use crate::{parser::Parse, ProtocolVersion};

    use super::*;

    fn encode(packet: Packet<'_>) -> String {
        match CommonParser::default().serialize(packet).0 {
            TransportPayload::Str(d) => d.into(),
            TransportPayload::Bytes(_) => panic!("testing only returns str"),
        }
    }
    fn decode(value: String) -> Packet<'static> {
        CommonParser::default().parse_str(value.into()).unwrap()
    }

    #[test]
    fn packet_decode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = decode(payload);

        assert_eq!(Packet::connect("/", sid, ProtocolVersion::V5), packet);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet = decode(payload);

        assert_eq!(Packet::connect("/admin™", sid, ProtocolVersion::V5), packet);
    }

    #[test]
    fn packet_encode_connect() {
        let sid = Sid::new();
        let payload = format!("0{}", json!({ "sid": sid }));
        let packet = encode(Packet::connect("/", sid, ProtocolVersion::V5));
        assert_eq!(packet, payload);

        let payload = format!("0/admin™,{}", json!({ "sid": sid }));
        let packet: String = encode(Packet::connect("/admin™", sid, ProtocolVersion::V5));
        assert_eq!(packet, payload);
    }

    // Disconnect,

    #[test]
    fn packet_decode_disconnect() {
        let payload = "1".to_string();
        let packet = decode(payload);
        assert_eq!(Packet::disconnect("/"), packet);

        let payload = "1/admin™,".to_string();
        let packet = decode(payload);
        assert_eq!(Packet::disconnect("/admin™"), packet);
    }

    #[test]
    fn packet_encode_disconnect() {
        let payload = "1".to_string();
        let packet = encode(Packet::disconnect("/"));
        assert_eq!(packet, payload);

        let payload = "1/admin™,".to_string();
        let packet = encode(Packet::disconnect("/admin™"));
        assert_eq!(packet, payload);
    }

    // Event(String, Value, Option<i64>),
    #[test]
    fn packet_decode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value" }]));
        let packet = decode(payload);

        assert_eq!(
            Packet::event("/", "event", json!([{"data": "value"}])),
            packet
        );

        // Check with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value" }]));
        let packet = decode(payload);

        let mut comparison_packet = Packet::event("/", "event", json!([{"data": "value"}]));
        comparison_packet.inner.set_ack_id(1);
        assert_eq!(packet, comparison_packet);

        // Check with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = decode(payload);

        assert_eq!(
            Packet::event("/admin™", "event", json!([{"data": "value™"}])),
            packet
        );

        // Check with ack ID and NS
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = decode(payload);
        packet.inner.set_ack_id(1);

        let mut comparison_packet = Packet::event("/admin™", "event", json!([{"data": "value™"}]));
        comparison_packet.inner.set_ack_id(1);

        assert_eq!(packet, comparison_packet);
    }

    #[test]
    fn packet_encode_event() {
        let payload = format!("2{}", json!(["event", { "data": "value™" }]));
        let packet = encode(Packet::event("/", "event", json!({ "data": "value™" })));

        assert_eq!(packet, payload);

        // Encode empty data
        let payload = format!("2{}", json!(["event", []]));
        let packet = encode(Packet::event("/", "event", json!([])));

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("21{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event("/", "event", json!({ "data": "value™" }));
        packet.inner.set_ack_id(1);
        let packet = encode(packet);

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("2/admin™,{}", json!(["event", { "data": "value™" }]));
        let packet = encode(Packet::event("/admin™", "event", json!({"data": "value™"})));

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("2/admin™,1{}", json!(["event", { "data": "value™" }]));
        let mut packet = Packet::event("/admin™", "event", json!([{"data": "value™"}]));
        packet.inner.set_ack_id(1);
        let packet = encode(packet);
        assert_eq!(packet, payload);
    }

    // EventAck(Value, i64),
    #[test]
    fn packet_decode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet = decode(payload);

        assert_eq!(Packet::ack("/", json!(["data"]), 54), packet);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = decode(payload);

        assert_eq!(Packet::ack("/admin™", json!(["data"]), 54), packet);
    }

    #[test]
    fn packet_encode_event_ack() {
        let payload = "354[\"data\"]".to_string();
        let packet = encode(Packet::ack("/", json!("data"), 54));
        assert_eq!(packet, payload);

        let payload = "3/admin™,54[\"data\"]".to_string();
        let packet = encode(Packet::ack("/admin™", json!("data"), 54));
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_encode_connect_error() {
        let payload = format!("4{}", json!({ "message": "Invalid namespace" }));
        let packet = encode(Packet::connect_error("/", "Invalid namespace"));
        assert_eq!(packet, payload);

        let payload = format!("4/admin™,{}", json!({ "message": "Invalid namespace" }));
        let packet = encode(Packet::connect_error("/admin™", "Invalid namespace"));
        assert_eq!(packet, payload);
    }

    // BinaryEvent(String, BinaryPacket, Option<i64>),
    #[test]
    fn packet_encode_binary_event() {
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("51-{}", json);
        let packet = encode(Packet::bin_event(
            "/",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        ));

        assert_eq!(packet, payload);

        // Encode with ack ID
        let payload = format!("51-254{}", json);
        let mut packet = Packet::bin_event(
            "/",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("51-/admin™,{}", json);
        let packet = encode(Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![Bytes::from_static(&[1])],
        ));

        assert_eq!(packet, payload);

        // Encode with NS and ack ID
        let payload = format!("51-/admin™,254{}", json);
        let mut packet = Packet::bin_event(
            "/admin™",
            "event",
            json!([{"data": "value™"}]),
            vec![Bytes::from_static(&[1])],
        );
        packet.inner.set_ack_id(254);
        let packet = encode(packet);
        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_event() {
        let json = json!(["event", { "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryEvent(
                "event".into(),
                BinaryPacket {
                    bin: vec![Bytes::from_static(&[1])],
                    data: json!([{"data": "value™"}]),
                    payload_count: 1,
                },
                ack,
            ),
            ns: ns.into(),
        };
        let parser = CommonParser::default();
        let payload = format!("51-{}", json);
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(None, "/"));

        // Check with ack ID
        let parser = CommonParser::default();
        let payload = format!("51-254{}", json);
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/"));

        // Check with NS
        let parser = CommonParser::default();
        let payload = format!("51-/admin™,{}", json);
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(None, "/admin™"));

        // Check with ack ID and NS
        let parser = CommonParser::default();
        let payload = format!("51-/admin™,254{}", json);
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(Some(254), "/admin™"));
    }

    // BinaryAck(BinaryPacket, i64),
    #[test]
    fn packet_encode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);

        let payload = format!("61-54{}", json);
        let packet = encode(Packet::bin_ack(
            "/",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
            54,
        ));

        assert_eq!(packet, payload);

        // Encode with NS
        let payload = format!("61-/admin™,54{}", json);
        let packet = encode(Packet::bin_ack(
            "/admin™",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
            54,
        ));

        assert_eq!(packet, payload);
    }

    #[test]
    fn packet_decode_binary_ack() {
        let json = json!([{ "data": "value™" }, { "_placeholder": true, "num": 0}]);
        let comparison_packet = |ack, ns: &'static str| Packet {
            inner: PacketData::BinaryAck(
                BinaryPacket {
                    bin: vec![Bytes::from_static(&[1])],
                    data: json!([{"data": "value™"}]),
                    payload_count: 1,
                },
                ack,
            ),
            ns: ns.into(),
        };

        let payload = format!("61-54{}", json);
        let parser = CommonParser::default();
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();

        assert_eq!(packet, comparison_packet(54, "/"));

        // Check with NS
        let parser = CommonParser::default();
        let payload = format!("61-/admin™,54{}", json);
        assert!(matches!(
            parser.parse_str(payload.into()),
            Err(Error::NeedsMoreBinaryData)
        ));
        let packet = parser.parse_bin(Bytes::from_static(&[1])).unwrap();
        assert_eq!(packet, comparison_packet(54, "/admin™"));
    }

    #[test]
    fn packet_size_hint() {
        let sid = Sid::new();
        let packet = Packet::connect("/", sid, ProtocolVersion::V5);
        assert_eq!(get_size_hint(&packet), 1);

        let packet = Packet::connect("/admin", sid, ProtocolVersion::V5);
        assert_eq!(get_size_hint(&packet), 8);

        let packet = Packet::connect("admin", sid, ProtocolVersion::V4);
        assert_eq!(get_size_hint(&packet), 8);

        let packet = Packet::disconnect("/");
        assert_eq!(get_size_hint(&packet), 1);

        let packet = Packet::disconnect("/admin");
        assert_eq!(get_size_hint(&packet), 8);

        let packet = Packet::event("/", "event", json!({ "data": "value™" }));
        assert_eq!(get_size_hint(&packet), 1);

        let packet = Packet::event("/admin", "event", json!({ "data": "value™" }));
        assert_eq!(get_size_hint(&packet), 8);

        let packet = Packet::ack("/", json!("data"), 54);
        assert_eq!(get_size_hint(&packet), 3);

        let packet = Packet::ack("/admin", json!("data"), 54);
        assert_eq!(get_size_hint(&packet), 10);

        let packet = Packet::bin_event(
            "/",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        );
        assert_eq!(get_size_hint(&packet), 3);

        let packet = Packet::bin_event(
            "/admin",
            "event",
            json!({ "data": "value™" }),
            vec![Bytes::from_static(&[1])],
        );
        assert_eq!(get_size_hint(&packet), 10);

        let packet = Packet::bin_ack("/", json!("data"), vec![Bytes::from_static(&[1])], 54);
        assert_eq!(get_size_hint(&packet), 5);
    }
}
