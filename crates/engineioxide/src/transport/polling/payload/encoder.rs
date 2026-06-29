//! ## Encoder for http payloads
//!
//! There is 3 different encoders:
//! * engine.io v4 encoder
//! * engine.io v3 encoder:
//!    * string encoder (used when there is no binary packet or when the client does not support binary)
//!    * binary encoder (used when there are binary packets and the client supports binary)
//!

use tokio::sync::MutexGuard;

use crate::{
    errors::Error, packet::Packet, peekable::PeekableReceiver, socket::PacketBuf,
    transport::polling::payload::Payload,
};

/// Try to immediately poll a new packet buf from the rx channel and check that the new packet can be added to the payload
///
/// Manually close the channel if the packet is a close packet
/// It will allow to notify the [`Socket`](crate::socket::Socket) that the session is closed
///
/// ## Arguments
/// * `rx` - The channel to poll
/// * `payload_len` - The current payload length
/// * `max_payload` - The maximum payload length
/// * `b64` - If binary packets should be encoded in base64
fn try_recv_packet(
    rx: &mut MutexGuard<'_, PeekableReceiver<PacketBuf>>,
    payload_len: usize,
    max_payload: u64,
    b64: bool,
) -> Option<PacketBuf> {
    if let Some(packets) = rx.peek() {
        let size = packets.iter().map(|p| p.get_size_hint(b64)).sum::<usize>();
        if (payload_len + size) as u64 > max_payload {
            #[cfg(feature = "tracing")]
            tracing::debug!("payload too big, stopping encoding for this payload");
            return None;
        }
    }

    let packets = rx.try_recv().ok();

    if Some(&Packet::Close) == packets.as_ref().and_then(|p| p.first()) {
        #[cfg(feature = "tracing")]
        tracing::debug!("Received close packet, closing channel");
        rx.try_recv().ok();
        rx.close();
    }

    #[cfg(feature = "tracing")]
    tracing::debug!("sending packet: {:?}", packets);
    packets
}

/// Same as [`try_recv_packet`]
/// but wait for a new packet if there is no packet in the buffer
async fn recv_packet(
    rx: &mut MutexGuard<'_, PeekableReceiver<PacketBuf>>,
) -> Result<PacketBuf, Error> {
    let packet = rx.recv().await.ok_or(Error::Aborted)?;
    if Some(&Packet::Close) == packet.first() {
        #[cfg(feature = "tracing")]
        tracing::debug!("Received close packet, closing channel");
        rx.close();
    }

    #[cfg(feature = "tracing")]
    tracing::debug!("sending packet: {:?}", packet);
    Ok(packet)
}

/// Encode multiple packets into a string payload according to the
/// [engine.io v4 protocol](https://socket.io/fr/docs/v4/engine-io-protocol/#http-long-polling-1)
pub async fn v4_encoder(
    mut rx: MutexGuard<'_, PeekableReceiver<PacketBuf>>,
    max_payload: u64,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) -> Result<Payload, Error> {
    use crate::transport::polling::payload::PACKET_SEPARATOR_V4;

    #[cfg(feature = "tracing")]
    tracing::debug!("encoding payload with v4 encoder");
    let mut data: String = String::new();

    // Encode any pending volatile packets first so they are included in
    // the current response even if the main channel has a backlog.
    // The watch receiver is checked again at each iteration of the main
    // channel drain loop, so volatile packets arriving during encoding
    // (between .await points) are also captured.
    encode_volatile_packets_v4(&mut data, volatile_rx);

    // Send all packets in the buffer
    const PUNCTUATION_LEN: usize = 1;
    loop {
        // Check for volatile data before each main channel read
        encode_volatile_packets_v4(&mut data, volatile_rx);

        let Some(packets) =
            try_recv_packet(&mut rx, data.len() + PUNCTUATION_LEN, max_payload, true)
        else {
            break;
        };
        for packet in packets {
            let packet: String = packet.into();

            if !data.is_empty() {
                data.push(std::char::from_u32(PACKET_SEPARATOR_V4 as u32).unwrap());
            }
            data.push_str(&packet);
        }
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packets = recv_packet(&mut rx).await?;
        for packet in packets {
            if !data.is_empty() {
                data.push(std::char::from_u32(PACKET_SEPARATOR_V4 as u32).unwrap());
            }

            let packet: String = packet.into();
            data.push_str(&packet);
        }

        // Check for volatile packets that arrived during the parked recv
        let mut volatile_data = String::new();
        encode_volatile_packets_v4(&mut volatile_data, volatile_rx);
        if !volatile_data.is_empty() {
            if !data.is_empty() {
                volatile_data.push(std::char::from_u32(PACKET_SEPARATOR_V4 as u32).unwrap());
            }
            volatile_data.push_str(&data);
            data = volatile_data;
        }
    }

    Ok(Payload::new(data.into(), false))
}

/// Encode any pending volatile packets from the watch receiver into the
/// v4 payload string. The watch is checked lazily (only when new data is
/// available) so volatile packets arriving during encoding are captured.
fn encode_volatile_packets_v4(
    data: &mut String,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) {
    use crate::transport::polling::payload::PACKET_SEPARATOR_V4;

    if !volatile_rx.has_changed().unwrap_or(false) {
        return;
    }
    let value = volatile_rx.borrow_and_update().clone();
    if let Some(packets) = value {
        for packet in packets {
            let packet: String = packet.into();
            if !data.is_empty() {
                data.push(std::char::from_u32(PACKET_SEPARATOR_V4 as u32).unwrap());
            }
            data.push_str(&packet);
        }
    }
}

/// Encode one packet into a *binary* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub fn v3_bin_packet_encoder(packet: Packet, data: &mut bytes::BytesMut) {
    use crate::transport::polling::payload::BINARY_PACKET_SEPARATOR_V3;
    use bytes::BufMut;

    let mut itoa = itoa::Buffer::new();
    match packet {
        Packet::BinaryV3(bin) => {
            let len = itoa.format(bin.len() + 1);
            let len_len = len.len(); // len is guaranteed to be ascii

            data.reserve(1 + len_len + 2 + bin.len());

            data.put_u8(0x1); // 1 = binary
            for char in len.chars() {
                data.put_u8(char as u8 - 48);
            }
            data.put_u8(BINARY_PACKET_SEPARATOR_V3); // separator
            data.put_u8(0x04); // message packet type
            data.extend_from_slice(&bin); // raw data
        }
        packet => {
            let packet: String = packet.into();
            let len = itoa.format(packet.len());
            let len_len = len.len(); // len is guaranteed to be ascii

            data.reserve(1 + len_len + 1 + packet.len());

            data.put_u8(0x0); // 0 = string
            for char in len.chars() {
                data.put_u8(char as u8 - 48);
            }
            data.put_u8(BINARY_PACKET_SEPARATOR_V3); // separator
            data.extend_from_slice(packet.as_bytes()); // packet
        }
    };
}

/// Encode one packet into a *string* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub fn v3_string_packet_encoder(packet: Packet, data: &mut bytes::BytesMut) {
    use crate::transport::polling::payload::STRING_PACKET_SEPARATOR_V3;
    use bytes::BufMut;
    let packet: String = packet.into();
    let packet = format!(
        "{}{}{}",
        packet.encode_utf16().count(), // The protocol uses in UTF16 code points for packet size because of JS
        STRING_PACKET_SEPARATOR_V3 as char,
        packet
    );
    data.put_slice(packet.as_bytes());
}

/// Encode multiple packet packet into a *string* payload if there is no binary packet or into a *binary* payload if there are binary packets
/// according to the [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub async fn v3_binary_encoder(
    mut rx: MutexGuard<'_, PeekableReceiver<PacketBuf>>,
    max_payload: u64,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) -> Result<Payload, Error> {
    let mut data = bytes::BytesMut::new();
    let mut packet_buffer: Vec<Packet> = Vec::new();

    // estimated size of the `packet_buffer` in bytes
    let mut estimated_size: usize = 0;
    // number of digits of the max packet size, used to approximate the payload size
    let max_packet_size_len = max_payload.checked_ilog10().unwrap_or(0) as usize + 1;

    #[cfg(feature = "tracing")]
    tracing::debug!("encoding payload with v3 binary encoder");
    // buffer all packets to find if there is binary packets
    let mut has_binary = false;

    // Encode any pending volatile packets first. Checked again inside the
    // main drain loop so volatile packets arriving during encoding are captured.
    buffer_volatile_packets(
        &mut packet_buffer,
        &mut estimated_size,
        &mut has_binary,
        max_packet_size_len,
        volatile_rx,
    );

    loop {
        buffer_volatile_packets(
            &mut packet_buffer,
            &mut estimated_size,
            &mut has_binary,
            max_packet_size_len,
            volatile_rx,
        );

        let Some(packets) = try_recv_packet(&mut rx, estimated_size, max_payload, false) else {
            break;
        };
        for packet in packets {
            if packet.is_binary() {
                has_binary = true;
            }

            const PUNCTUATION_LEN: usize = 2;
            estimated_size += packet.get_size_hint(false) + max_packet_size_len + PUNCTUATION_LEN;

            packet_buffer.push(packet);
        }
    }

    if has_binary {
        for packet in packet_buffer.drain(..) {
            v3_bin_packet_encoder(packet, &mut data);
        }
    } else {
        for packet in packet_buffer.drain(..) {
            v3_string_packet_encoder(packet, &mut data);
        }
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packets = recv_packet(&mut rx).await?;
        has_binary = packets.iter().any(|p| p.is_binary()) || has_binary;

        // Check for volatile that arrived during the park
        buffer_volatile_packets(
            &mut packet_buffer,
            &mut estimated_size,
            &mut has_binary,
            max_packet_size_len,
            volatile_rx,
        );

        for packet in packet_buffer.drain(..) {
            if has_binary {
                v3_bin_packet_encoder(packet, &mut data);
            } else {
                v3_string_packet_encoder(packet, &mut data);
            }
        }
        for packet in packets {
            if has_binary {
                v3_bin_packet_encoder(packet, &mut data);
            } else {
                v3_string_packet_encoder(packet, &mut data);
            }
        }
    }

    #[cfg(feature = "tracing")]
    tracing::debug!("sending packet: {:?}", &data);
    Ok(Payload::new(data.freeze(), has_binary))
}

/// Buffer any pending volatile packets from the watch receiver into the
/// packet buffer. Called at each iteration so volatile packets arriving
/// during encoding (between .await points) are captured.
#[cfg(feature = "v3")]
fn buffer_volatile_packets(
    packet_buffer: &mut Vec<Packet>,
    estimated_size: &mut usize,
    has_binary: &mut bool,
    max_packet_size_len: usize,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) {
    if !volatile_rx.has_changed().unwrap_or(false) {
        return;
    }
    let value = volatile_rx.borrow_and_update().clone();
    if let Some(packets) = value {
        for packet in packets {
            if packet.is_binary() {
                *has_binary = true;
            }
            const PUNCTUATION_LEN: usize = 2;
            *estimated_size += packet.get_size_hint(false) + max_packet_size_len + PUNCTUATION_LEN;
            packet_buffer.push(packet);
        }
    }
}

/// Encode multiple packet packet into a *string* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub async fn v3_string_encoder(
    mut rx: MutexGuard<'_, PeekableReceiver<PacketBuf>>,
    max_payload: u64,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) -> Result<Payload, Error> {
    let mut data = bytes::BytesMut::new();

    #[cfg(feature = "tracing")]
    tracing::debug!("encoding payload with v3 string encoder");

    const PUNCTUATION_LEN: usize = 2;
    // number of digits of the max packet size, used to approximate the payload size
    let max_packet_size_len = max_payload.checked_ilog10().unwrap_or(0) as usize + 1;

    // Encode any pending volatile packets first; checked again in the main
    // drain loop so volatile packets arriving during encoding are captured.
    encode_volatile_v3_string(&mut data, volatile_rx);

    loop {
        encode_volatile_v3_string(&mut data, volatile_rx);

        let Some(packets) = try_recv_packet(
            &mut rx,
            data.len() + PUNCTUATION_LEN + max_packet_size_len,
            max_payload,
            true,
        ) else {
            break;
        };
        for packet in packets {
            v3_string_packet_encoder(packet, &mut data);
        }
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packets = recv_packet(&mut rx).await?;
        for packet in packets {
            v3_string_packet_encoder(packet, &mut data);
        }

        let mut volatile_data = bytes::BytesMut::new();
        encode_volatile_v3_string(&mut volatile_data, volatile_rx);
        if !volatile_data.is_empty() {
            volatile_data.unsplit(data);
            data = volatile_data;
        }
    }

    Ok(Payload::new(data.freeze(), false))
}

/// Encode any pending volatile packets from the watch receiver into the
/// v3 string payload. Called at each iteration so volatile packets
/// arriving during encoding are captured.
#[cfg(feature = "v3")]
fn encode_volatile_v3_string(
    data: &mut bytes::BytesMut,
    volatile_rx: &mut tokio::sync::watch::Receiver<Option<PacketBuf>>,
) {
    if !volatile_rx.has_changed().unwrap_or(false) {
        return;
    }
    let value = volatile_rx.borrow_and_update().clone();
    if let Some(packets) = value {
        for packet in packets {
            v3_string_packet_encoder(packet, data);
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::sync::Mutex;

    use PacketBuf;

    use super::*;
    const MAX_PAYLOAD: u64 = 100_000;

    fn dummy_volatile_rx() -> tokio::sync::watch::Receiver<Option<PacketBuf>> {
        let (_, rx) = tokio::sync::watch::channel(None);
        rx
    }

    #[tokio::test]
    async fn encode_v4_payload() {
        let mut vr = dummy_volatile_rx();
        const PAYLOAD: &str = "4hello€\x1ebAQIDBA==\x1e4hello€";
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let rx = Mutex::new(PeekableReceiver::new(rx));
        let rx = rx.lock().await;
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Binary(Bytes::from_static(&[
            1, 2, 3, 4
        ]))])
        .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, PAYLOAD.as_bytes());
    }

    #[tokio::test]
    async fn encode_v4_payload_parked_poll_multi_packet_batch() {
        let mut vr = dummy_volatile_rx();
        const PAYLOAD: &str = "4hello€\x1ebAQIDBA==";
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let rx = Mutex::new(PeekableReceiver::new(rx));
        let rx = rx.lock().await;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.try_send(smallvec::smallvec![
                Packet::Message("hello€".into()),
                Packet::Binary(Bytes::from_static(&[1, 2, 3, 4]))
            ])
            .unwrap();
        });
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, PAYLOAD);
    }

    #[tokio::test]
    async fn max_payload_v4() {
        let mut vr = dummy_volatile_rx();
        const MAX_PAYLOAD: u64 = 10;
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Binary(Bytes::from_static(&[
            1, 2, 3, 4
        ]))])
        .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
            assert_eq!(data, "4hello€".as_bytes());
        }
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD + 10, &mut vr).await.unwrap();
            assert_eq!(data, "bAQIDBA==\x1e4hello€".as_bytes());
        }
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD + 10, &mut vr).await.unwrap();
            assert_eq!(data, "4hello€".as_bytes());
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3_string_payload_utf16_length() {
        let mut vr = dummy_volatile_rx();
        // Length must be the number of UTF-16 code units to match the
        // engine.io v3 JS reference implementation. The message "4𝕊"
        // (packet type '4' + non-BMP codepoint U+1D54A) has 2 codepoints
        // but 3 UTF-16 code units.
        const PAYLOAD: &str = "3:4𝕊";
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let rx = mutex.lock().await;
        tx.try_send(smallvec::smallvec![Packet::Message("𝕊".into())])
            .unwrap();
        let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, PAYLOAD.as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3b64_payload() {
        let mut vr = dummy_volatile_rx();
        const PAYLOAD: &str = "7:4hello€10:b4AQIDBA==7:4hello€";
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let rx = mutex.lock().await;

        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::BinaryV3(Bytes::from_static(
            &[1, 2, 3, 4]
        ))])
        .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        let Payload {
            data, has_binary, ..
        } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, PAYLOAD.as_bytes());
        assert!(!has_binary);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn max_payload_v3_b64() {
        let mut vr = dummy_volatile_rx();
        const MAX_PAYLOAD: u64 = 10;

        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::BinaryV3(Bytes::from_static(
            &[1, 2, 3, 4]
        ))])
        .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
            assert_eq!(data, "7:4hello€".as_bytes());
        }
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD + 10, &mut vr)
                .await
                .unwrap();
            assert_eq!(data, "10:b4AQIDBA==".as_bytes());
        }
        {
            // Next call drains one of the remaining Message packets.
            let rx = mutex.lock().await;
            let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD + 10, &mut vr)
                .await
                .unwrap();
            assert_eq!(data, "7:4hello€".as_bytes());
        }
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3binary_payload() {
        let mut vr = dummy_volatile_rx();
        const PAYLOAD: [u8; 20] = [
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let rx = mutex.lock().await;

        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::BinaryV3(Bytes::from_static(
            &[1, 2, 3, 4]
        ))])
        .unwrap();
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(*data, PAYLOAD);
        assert!(has_binary);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3binary_payload_parked_poll_multi_packet_batch() {
        let mut vr = dummy_volatile_rx();
        // When the v3 binary encoder is parked on an empty buffer and a
        // multi-packet batch arrives, every packet must be encoded with the
        // same framing (binary framing if any packet in the batch is binary).
        // Otherwise the payload mixes string- and binary-framed packets while
        // being flagged as binary.
        const PAYLOAD: [u8; 20] = [
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let rx = mutex.lock().await;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.try_send(smallvec::smallvec![
                Packet::Message("hello€".into()),
                Packet::BinaryV3(Bytes::from_static(&[1, 2, 3, 4]))
            ])
            .unwrap();
        });
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(*data, PAYLOAD);
        assert!(has_binary);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn max_payload_v3_binary() {
        let mut vr = dummy_volatile_rx();
        const MAX_PAYLOAD: u64 = 25;

        const PAYLOAD: [u8; 23] = [
            0, 1, 1, 255, 52, 104, 101, 108, 108, 111, 111, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2,
            3, 4,
        ];
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("hellooo€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::BinaryV3(Bytes::from_static(
            &[1, 2, 3, 4]
        ))])
        .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("hello€".into())])
            .unwrap();
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
            assert_eq!(*data, PAYLOAD);
        }
        {
            let rx = mutex.lock().await;
            let Payload { data, .. } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
            assert_eq!(data, "7:4hello€7:4hello€".as_bytes());
        }
    }

    fn make_volatile_chan(
        packets: PacketBuf,
    ) -> (
        tokio::sync::watch::Sender<Option<PacketBuf>>,
        tokio::sync::watch::Receiver<Option<PacketBuf>>,
    ) {
        let (tx, rx) = tokio::sync::watch::channel(None);
        tx.send(Some(packets)).unwrap();
        (tx, rx)
    }

    #[tokio::test]
    async fn v4_volatile_before_normal() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("foo".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("bar".into())])
            .unwrap();
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("v1".into())]);

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4v1\x1e4foo\x1e4bar".as_bytes());
    }

    #[tokio::test]
    async fn v4_normal_before_volatile() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("foo".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("bar".into())])
            .unwrap();
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("v1".into())]);

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4v1\x1e4foo\x1e4bar".as_bytes());
    }

    #[tokio::test]
    async fn v4_volatile_overwrite() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("normal".into())])
            .unwrap();
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("dropped".into())]))
            .unwrap();
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("kept".into())]))
            .unwrap();

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4kept\x1e4normal".as_bytes());
    }

    #[tokio::test]
    async fn v4_volatile_mid_encoding() {
        let (main_tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        main_tx
            .try_send(smallvec::smallvec![Packet::Message("first".into())])
            .unwrap();
        main_tx
            .try_send(smallvec::smallvec![Packet::Message("second".into())])
            .unwrap();

        let rx = mutex.lock().await;
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("mid".into())]))
            .unwrap();

        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4mid\x1e4first\x1e4second".as_bytes());
    }

    #[tokio::test]
    async fn v4_volatile_only_no_drain() {
        let (_main_tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("v_only".into())]);
        drop(_main_tx);

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4v_only".as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_string_volatile_mixed() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("foo".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("bar".into())])
            .unwrap();
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("v1".into())]);

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "3:4v14:4foo4:4bar".as_bytes());
        assert!(!has_binary);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_string_volatile_overwrite() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("normal".into())])
            .unwrap();
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("drop1".into())]))
            .unwrap();
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("keep1".into())]))
            .unwrap();

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "6:4keep17:4normal".as_bytes());
        assert!(!has_binary);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_binary_volatile_mixed() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("foo".into())])
            .unwrap();
        let (_volatile_tx, mut vr) = make_volatile_chan(smallvec::smallvec![Packet::BinaryV3(
            Bytes::from_static(&[1, 2, 3, 4])
        )]);

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert!(has_binary);
        assert_eq!(
            &data[..8],
            &[0x01, 0x05, 0xff, 0x04, 0x01, 0x02, 0x03, 0x04][..]
        );
        assert_eq!(&data[8..], &[0x00, 0x04, 0xff, 0x34, 0x66, 0x6f, 0x6f][..]);
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_binary_volatile_overwrite() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("normal".into())])
            .unwrap();
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::Message("drop_v".into())]))
            .unwrap();
        volatile_tx
            .send(Some(smallvec::smallvec![Packet::BinaryV3(
                Bytes::from_static(&[9, 9])
            )]))
            .unwrap();

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert!(has_binary);
        assert_eq!(&data[..6], &[0x01, 0x03, 0xff, 0x04, 0x09, 0x09][..]);
        assert_eq!(
            &data[6..],
            &[0x00, 0x07, 0xff, 0x34, 0x6e, 0x6f, 0x72, 0x6d, 0x61, 0x6c][..]
        );
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_binary_volatile_determines_has_binary() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        tx.try_send(smallvec::smallvec![Packet::Message("foo".into())])
            .unwrap();
        let (_volatile_tx, mut vr) = make_volatile_chan(smallvec::smallvec![Packet::BinaryV3(
            Bytes::from_static(&[1, 2, 3])
        )]);

        let rx = mutex.lock().await;
        let Payload {
            data: _,
            has_binary,
            ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert!(has_binary);
    }

    #[tokio::test]
    async fn v4_volatile_arrives_during_parked_poll() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        let volatile_tx = std::sync::Arc::new(std::sync::Mutex::new(volatile_tx));

        let tx_clone = tx.clone();
        let vt = volatile_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            vt.lock()
                .unwrap()
                .send(Some(smallvec::smallvec![Packet::Message(
                    "volatile".into()
                )]))
                .unwrap();
            tx_clone
                .try_send(smallvec::smallvec![Packet::Message("normal".into())])
                .unwrap();
        });

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        // After the fix: volatile arrived during park, now captured same payload
        assert_eq!(data, "4volatile\x1e4normal".as_bytes());

        drop(tx);
        let rx = mutex.lock().await;
        let result = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await;
        assert!(result.is_err()); // no more data
    }

    #[tokio::test]
    async fn v4_volatile_pushes_past_max_payload() {
        const SMALL_LIMIT: u64 = 12;
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("big_vol".into())]);
        tx.try_send(smallvec::smallvec![Packet::Message("normal".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("extra".into())])
            .unwrap();

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, SMALL_LIMIT, &mut vr).await.unwrap();
        assert_eq!(data, "4big_vol".as_bytes());

        let rx = mutex.lock().await;
        let Payload { data, .. } = v4_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "4normal\x1e4extra".as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_string_volatile_during_parked_poll() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        let volatile_tx = std::sync::Arc::new(std::sync::Mutex::new(volatile_tx));

        let tx_clone = tx.clone();
        let vt = volatile_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            vt.lock()
                .unwrap()
                .send(Some(smallvec::smallvec![Packet::Message("v".into())]))
                .unwrap();
            tx_clone
                .try_send(smallvec::smallvec![Packet::Message("n".into())])
                .unwrap();
        });

        let rx = mutex.lock().await;
        let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "2:4v2:4n".as_bytes());

        drop(tx);
        let rx = mutex.lock().await;
        let result = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await;
        assert!(result.is_err());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_string_volatile_max_payload() {
        const SMALL_LIMIT: u64 = 12;
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("big".into())]);
        tx.try_send(smallvec::smallvec![Packet::Message("normal".into())])
            .unwrap();
        tx.try_send(smallvec::smallvec![Packet::Message("extra".into())])
            .unwrap();

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_string_encoder(rx, SMALL_LIMIT, &mut vr).await.unwrap();
        assert_eq!(data, "4:4big".as_bytes());
        assert!(!has_binary);

        let rx = mutex.lock().await;
        let Payload { data, .. } = v3_string_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "7:4normal6:4extra".as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_binary_volatile_during_parked_poll() {
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (volatile_tx, mut vr) = tokio::sync::watch::channel(None);
        let volatile_tx = std::sync::Arc::new(std::sync::Mutex::new(volatile_tx));

        let tx_clone = tx.clone();
        let vt = volatile_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            vt.lock()
                .unwrap()
                .send(Some(smallvec::smallvec![Packet::BinaryV3(
                    Bytes::from_static(&[1, 2])
                )]))
                .unwrap();
            tx_clone
                .try_send(smallvec::smallvec![Packet::Message("n".into())])
                .unwrap();
        });

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        // After fix: volatile binary + normal message both captured, has_binary=true
        assert!(has_binary);
        // Volatile binary: 0x01 0x03 0xFF 0x04 0x01 0x02 (6 bytes)
        // Normal message in binary frame: 0x00 0x02 0xFF 0x34 0x6E (5 bytes)
        assert_eq!(data.len(), 11);
        assert_eq!(&data[..6], &[0x01, 0x03, 0xff, 0x04, 0x01, 0x02][..]);
        assert_eq!(&data[6..], &[0x00, 0x02, 0xff, 0x34, 0x6e][..]);

        drop(tx);
        let rx = mutex.lock().await;
        let result = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await;
        assert!(result.is_err());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn v3_binary_volatile_max_payload() {
        const SMALL_LIMIT: u64 = 15;
        let (tx, rx) = tokio::sync::mpsc::channel::<PacketBuf>(10);
        let mutex = Mutex::new(PeekableReceiver::new(rx));
        let (_volatile_tx, mut vr) =
            make_volatile_chan(smallvec::smallvec![Packet::Message("big_volatile".into())]);
        tx.try_send(smallvec::smallvec![Packet::Message("after".into())])
            .unwrap();

        let rx = mutex.lock().await;
        let Payload {
            data, has_binary, ..
        } = v3_binary_encoder(rx, SMALL_LIMIT, &mut vr).await.unwrap();
        assert!(!has_binary);
        assert_eq!(data, "13:4big_volatile".as_bytes());

        let rx = mutex.lock().await;
        let Payload { data, .. } = v3_binary_encoder(rx, MAX_PAYLOAD, &mut vr).await.unwrap();
        assert_eq!(data, "6:4after".as_bytes());
    }
}
