// We use VecDeque for binary attachments since it provides efficient front/back operations
// which is useful for streaming binary data
use std::collections::VecDeque;

// BytesMut provides a mutable buffer optimized for packet building
// BufMut adds convenient methods for writing to the buffer
use bytes::{BufMut, BytesMut};
use socketioxide_core::{
    packet::{Packet, PacketData},
    Str, Value,
};

/// Main packet serialization function that converts Socket.IO packets into a wire format
/// Returns a Value to maintain consistency with the Socket.IO protocol which can handle
/// both string and binary data
pub fn serialize_packet(packet: Packet) -> Value {
    // Pre-allocate buffer with exact size to avoid reallocations
    let capacity = get_size_hint(&packet);
    let mut buffer = BytesMut::with_capacity(capacity);

    // Socket.IO protocol requires packet type as first byte
    // We convert index to ASCII digit since packet types are 0-9
    buffer.put_u8(char::from_digit(packet.inner.index() as u32, 10).unwrap() as u8);

    // Match on packet type to handle different serialization requirements
    // Returns binary attachments separately to support Socket.IO binary events
    let bins = match &packet.inner {
        PacketData::Connect(Some(data)) => {
            // Connect packets can optionally include handshake data
            if let Some(data_str) = data.as_str() {
                // Only include namespace for non-binary packets per protocol
                if !packet.inner.is_binary() {
                    serialize_nsp(&mut buffer, &packet.ns);
                }
                buffer.put_slice(data_str.as_bytes());
                None
            } else {
                // Return empty on error to maintain protocol consistency
                return Value::Str(Str::copy_from_slice(""), None);
            }
        }
        PacketData::Disconnect | PacketData::Connect(None) => {
            // Simple packets just need namespace serialization
            if !packet.inner.is_binary() {
                serialize_nsp(&mut buffer, &packet.ns);
            }
            None
        }
        PacketData::Event(Value::Str(data, bins), ack) => {
            // Regular events include namespace, optional ack ID, and payload
            if !packet.inner.is_binary() {
                serialize_nsp(&mut buffer, &packet.ns);
            }
            serialize_ack(&mut buffer, *ack);
            serialize_data(&mut buffer, data);
            bins.clone()  // Clone binary attachments only when needed
        }
        PacketData::EventAck(Value::Str(data, bins), ack) => {
            // Acknowledgments are similar to events but always include ack ID
            if !packet.inner.is_binary() {
                serialize_nsp(&mut buffer, &packet.ns);
            }
            serialize_ack(&mut buffer, Some(*ack));
            serialize_data(&mut buffer, data);
            bins.clone()
        }
        PacketData::ConnectError(message) => {
            // Error packets wrap message in a standard JSON structure
            if !packet.inner.is_binary() {
                serialize_nsp(&mut buffer, &packet.ns);
            }
            #[derive(serde::Serialize)]
            struct ErrorMessage<'a> {
                message: &'a str,
            }
            if let Ok(data) = serde_json::to_vec(&ErrorMessage { message }) {
                buffer.put_slice(&data);
            }
            None
        }
        PacketData::BinaryEvent(Value::Str(data, bins), ack) => {
            // Binary events need attachment count prefix
            if let Some(bins_ref) = bins.as_ref() {
                serialize_attachments(&mut buffer, bins_ref.len());
            }
            serialize_nsp(&mut buffer, &packet.ns);
            serialize_ack(&mut buffer, *ack);
            serialize_data(&mut buffer, data);
            bins.clone()
        }
        PacketData::BinaryAck(Value::Str(data, bins), ack) => {
            // Binary acks similar to binary events
            if let Some(bins_ref) = bins.as_ref() {
                serialize_attachments(&mut buffer, bins_ref.len());
            }
            serialize_nsp(&mut buffer, &packet.ns);
            serialize_ack(&mut buffer, Some(*ack));
            serialize_data(&mut buffer, data);
            bins.clone()
        }
        _ => {
            // Return empty for unsupported packet types
            return Value::Str(Str::copy_from_slice(""), None);
        }
    };

    // Convert buffer to immutable bytes and wrap in Value with binary attachments
    let output = unsafe { Str::from_bytes_unchecked(buffer.freeze()) };
    Value::Str(output, bins)
}

// Helper functions keep the main logic clean and handle specific serialization tasks

/// Adds binary attachment count prefix for binary packets
fn serialize_attachments(buffer: &mut BytesMut, attachments: usize) {
    let mut itoa_buf = itoa::Buffer::new();
    buffer.put_slice(itoa_buf.format(attachments).as_bytes());
    buffer.put_u8(b'-');
}

/// Handles namespace serialization with proper formatting
/// Only adds leading slash if missing and includes comma separator
fn serialize_nsp(buffer: &mut BytesMut, nsp: &str) {
    if !nsp.is_empty() && nsp != "/" {
        if !nsp.starts_with('/') {
            buffer.put_u8(b'/');
        }
        buffer.put_slice(nsp.as_bytes());
        buffer.put_u8(b',');
    }
}

/// Serializes acknowledgment IDs when present
fn serialize_ack(buffer: &mut BytesMut, ack: Option<i64>) {
    let mut itoa_buf = itoa::Buffer::new();
    if let Some(ack) = ack {
        buffer.put_slice(itoa_buf.format(ack).as_bytes());
    }
}

/// Adds event payload data to the buffer
/// Data is already in correct format from earlier processing
fn serialize_data(buffer: &mut BytesMut, data: &str) {
    buffer.put_slice(data.as_bytes())
}

/// Calculates maximum possible serialized size for pre-allocation
/// This avoids buffer reallocations during serialization
fn get_size_hint(packet: &Packet) -> usize {
    use PacketData::*;
    
    // Constants for various protocol elements
    const PACKET_INDEX_SIZE: usize = 1;
    const BINARY_PUNCTUATION_SIZE: usize = 2;
    const ACK_PUNCTUATION_SIZE: usize = 1;
    const NS_PUNCTUATION_SIZE: usize = 1;

    // Calculate size based on packet type and content
    let data_size = match &packet.inner {
        Connect(Some(val)) => val.len(),
        Connect(None) => 0,
        Disconnect => 0,
        Event(Value::Str(data, _), ack) => {
            // Event size includes data and possible ack ID
            data.len()
                + ack
                    .and_then(i64::checked_ilog10)
                    .map(|s| s as usize + ACK_PUNCTUATION_SIZE)
                    .unwrap_or(0)
        }
        BinaryEvent(Value::Str(data, bin), ack) => {
            // Binary events need extra space for attachment count
            data.len()
                + ack
                    .and_then(i64::checked_ilog10)
                    .map(|s| s as usize + ACK_PUNCTUATION_SIZE)
                    .unwrap_or(0)
                + bin
                    .as_ref()
                    .map(VecDeque::len)
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
                    .map(VecDeque::len)
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

    // Add namespace size if needed
    let nsp_size = if packet.ns == "/" {
        0
    } else if packet.ns.starts_with('/') {
        packet.ns.len() + NS_PUNCTUATION_SIZE
    } else {
        packet.ns.len() + NS_PUNCTUATION_SIZE + 1 // Add 1 for leading slash
    };
    
    // Total size combines all components
    data_size + nsp_size + PACKET_INDEX_SIZE
}

// Tests validate size calculations and error handling
#[cfg(test)]
mod tests {
    // Test imports
    use bytes::Bytes;
    use serde_json::json;
    use socketioxide_core::{packet::ConnectPacket, Sid};
    use super::*;

    // Helper functions to create test values
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
        // Comprehensive tests verify size calculations match actual output
        // This ensures buffer pre-allocation is correct
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
        // Verify incorrect value types are caught
        serialize_packet(Packet::event("test", Value::Bytes(Bytes::new())));
    }
}