use bytes::{BufMut, BytesMut};
use socketioxide_core::{
    packet::{Packet, PacketData},
    Str, Value,
};

pub fn serialize_packet(packet: Packet) -> Value {
    let capacity = get_size_hint(&packet);
    let mut buffer = BytesMut::with_capacity(capacity);

    buffer.put_u8(char::from_digit(packet.inner.index() as u32, 10).unwrap() as u8);

    // Add the ns if it is not the default one and the packet is not binary
    // In case of bin packet, we should first add the payload count before ns
    if !packet.inner.is_binary() {
        serialize_nsp(&mut buffer, &packet.ns);
    }
    let bins = match packet.inner {
        PacketData::Connect(Some(data)) => {
            buffer.put_slice(data.as_str().unwrap().as_bytes());
            None
        }
        PacketData::Disconnect | PacketData::Connect(None) => None,
        PacketData::Event(Value::Str(data, bins), ack) => {
            serialize_ack(&mut buffer, ack);
            serialize_data(&mut buffer, &data);

            bins
        }
        PacketData::EventAck(Value::Str(data, bins), ack) => {
            serialize_ack(&mut buffer, Some(ack));
            serialize_data(&mut buffer, &data);
            bins
        }
        PacketData::ConnectError(ref message) => {
            #[derive(serde::Serialize)]
            struct ErrorMessage<'a> {
                message: &'a str,
            }
            let data = serde_json::to_vec(&ErrorMessage { message }).unwrap();
            buffer.put_slice(&data);
            None
        }
        PacketData::BinaryEvent(Value::Str(data, bins), ack) => {
            serialize_attachments(&mut buffer, bins.as_ref().map(Vec::len).unwrap_or(0));
            serialize_nsp(&mut buffer, &packet.ns);
            serialize_ack(&mut buffer, ack);

            serialize_data(&mut buffer, &data);
            bins
        }
        PacketData::BinaryAck(Value::Str(data, bins), ack) => {
            serialize_attachments(&mut buffer, bins.as_ref().map(Vec::len).unwrap_or(0));
            serialize_nsp(&mut buffer, &packet.ns);
            serialize_ack(&mut buffer, Some(ack));

            serialize_data(&mut buffer, &data);
            bins
        }
        _ => panic!("unexpected packet type"),
    };

    let output = unsafe { Str::from_bytes_unchecked(buffer.freeze()) };
    Value::Str(output, bins)
}

fn serialize_attachments(buffer: &mut BytesMut, attachments: usize) {
    let mut itoa_buf = itoa::Buffer::new();
    buffer.put_slice(itoa_buf.format(attachments).as_bytes());
    buffer.put_u8(b'-');
}
fn serialize_nsp(buffer: &mut BytesMut, nsp: &str) {
    if !nsp.is_empty() && nsp != "/" {
        if !nsp.starts_with('/') {
            buffer.put_u8(b'/');
        }
        buffer.put_slice(nsp.as_bytes());
        buffer.put_u8(b',');
    }
}
fn serialize_ack(buffer: &mut BytesMut, ack: Option<i64>) {
    let mut itoa_buf = itoa::Buffer::new();
    if let Some(ack) = ack {
        buffer.put_slice(itoa_buf.format(ack).as_bytes());
    }
}

/// Serialize event data in place to the following form: `[event, ...data]` if data is an array.
/// Or `[event, data]` if data is not an array.
/// ## Params:
/// - `event`: the event name
/// - `data`: preserialized msgpack data
/// - `buff`: a output buffer to write into
fn serialize_data(buffer: &mut BytesMut, data: &str) {
    buffer.put_slice(data.as_bytes())
}

/// Get the max size the packet could have when serialized
/// This is used to pre-allocate a buffer for the packet
fn get_size_hint(packet: &Packet) -> usize {
    use PacketData::*;
    const PACKET_INDEX_SIZE: usize = 1;
    const BINARY_PUNCTUATION_SIZE: usize = 2;
    const ACK_PUNCTUATION_SIZE: usize = 1;
    const NS_PUNCTUATION_SIZE: usize = 1;

    let data_size = match &packet.inner {
        Connect(Some(val)) => val.len(),
        Connect(None) => 0,
        Disconnect => 0,
        Event(Value::Str(data, _), ack) => {
            data.len()
                + ack
                    .and_then(i64::checked_ilog10)
                    .map(|s| s as usize + ACK_PUNCTUATION_SIZE)
                    .unwrap_or(0)
        }
        BinaryEvent(Value::Str(data, bin), ack) => {
            data.len()
                + ack
                    .and_then(i64::checked_ilog10)
                    .map(|s| s as usize + ACK_PUNCTUATION_SIZE)
                    .unwrap_or(0)
                + bin
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or(0)
                    .checked_ilog10()
                    .unwrap_or(0) as usize
                + BINARY_PUNCTUATION_SIZE
        }
        EventAck(Value::Str(data, _), ack) => {
            data.len() + ack.checked_ilog10().unwrap_or(0) as usize + ACK_PUNCTUATION_SIZE
        }
        BinaryAck(Value::Str(data, bins), ack) => {
            data.len()
                + ack.checked_ilog10().unwrap_or(0) as usize
                + bins
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or(0)
                    .checked_ilog10()
                    .unwrap_or(0) as usize
                + ACK_PUNCTUATION_SIZE
                + BINARY_PUNCTUATION_SIZE
        }
        ConnectError(data) => data.len() + "{\"message\":\"\"}".len(),
        data => unreachable!(
            "common parser should only serialize SocketIoValue::Str data: {:?}",
            data
        ),
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use serde_json::json;
    use socketioxide_core::{packet::ConnectPacket, Sid};

    use super::*;

    fn to_event_value(data: &impl serde::Serialize, event: &str) -> Value {
        crate::value::to_value(data, Some(event)).unwrap()
    }

    fn to_value(data: &impl serde::Serialize) -> Value {
        crate::value::to_value(data, None).unwrap()
    }
    fn to_connect_value(data: &impl serde::Serialize) -> Value {
        Value::Str(Str::from(serde_json::to_string(data).unwrap()), None)
    }

    #[test]
    fn packet_size_hint() {
        let sid = Sid::new();
        let value = to_connect_value(&ConnectPacket { sid });
        let packet = Packet::connect("/", Some(value.clone()));
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::connect("/admin", Some(value.clone()));
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::connect("admin", None);
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::connect_error("/", "test".to_string());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::connect_error("/admin", "test".to_string());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::disconnect("/");
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::disconnect("/admin");
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let basic_payload = to_event_value(&json!({ "data": "value™" }), "event");
        let basic_ack_payload = to_value(&json!("data"));
        let packet = Packet::event("/", basic_payload.clone());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::event("/admin", basic_payload.clone());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::ack("/", basic_ack_payload.clone(), 54);
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::ack("/admin", basic_ack_payload.clone(), 54);
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let bin_payload = to_event_value(
            &(json!({ "data": "value™" }), Bytes::from_static(&[1])),
            "event",
        );

        let bin_ack_payload = to_value(&(json!({ "data": "value™" }), Bytes::from_static(&[1])));
        let packet = Packet::event("/", bin_payload.clone());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::event("/admin", bin_payload.clone());
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::ack("/", bin_ack_payload.clone(), 54);
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());

        let packet = Packet::ack("/admin", bin_ack_payload.clone(), 54);
        assert_eq!(get_size_hint(&packet), serialize_packet(packet).len());
    }

    #[test]
    #[should_panic]
    fn panic_with_bad_value_type() {
        serialize_packet(Packet::event("test", Value::Bytes(Bytes::new())));
    }
}
