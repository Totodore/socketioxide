//! ## Encoder for http payloads
//!
//! There is 3 different encoders:
//! * engine.io v4 encoder
//! * engine.io v3 encoder:
//!    * string encoder (used when there is no binary packet or when the client does not support binary)
//!    * binary encoder (used when there is binary packets and the client supports binary)
//!

use tokio::sync::{mpsc::Receiver, MutexGuard};
use tracing::debug;

use crate::{errors::Error, packet::Packet};

/// Encode multiple packets into a string payload according to the
/// [engine.io v4 protocol](https://socket.io/fr/docs/v4/engine-io-protocol/#http-long-polling-1)
#[cfg(feature = "v4")]
pub async fn v4_encoder(mut rx: MutexGuard<'_, Receiver<Packet>>) -> Result<Vec<u8>, Error> {
    use crate::payload::PACKET_SEPARATOR_V4;

    debug!("encoding payload with v4 encoder");
    let mut data: String = String::new();

    // Send all packets in the buffer
    while let Ok(packet) = rx.try_recv() {
        debug!("sending packet: {:?}", packet);
        let packet: String = packet.try_into()?;

        if !data.is_empty() {
            data.push(std::char::from_u32(PACKET_SEPARATOR_V4 as u32).unwrap());
        }
        data.push_str(&packet);
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packet = rx.recv().await.ok_or(Error::Aborted)?;
        debug!("sending packet: {:?}", packet);
        let packet: String = packet.try_into()?;
        data.push_str(&packet);
    }
    Ok(data.into())
}

/// Encode one packet into a *binary* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub fn v3_bin_packet_encoder(packet: Packet, data: &mut Vec<u8>) -> Result<(), Error> {
    use crate::payload::BINARY_PACKET_SEPARATOR_V3;
    match packet {
        Packet::BinaryV3(bin) => {
            data.push(0x1);

            let len = (bin.len() + 1).to_string();
            for char in len.chars() {
                data.push(char as u8 - 48);
            }
            data.push(BINARY_PACKET_SEPARATOR_V3); // separator
            data.push(0x04); // message packet type
            data.extend_from_slice(&bin); // raw data
        }
        packet => {
            let packet: String = packet.try_into()?;
            data.push(0x0); // 0 = string

            let len = packet.len().to_string();
            for char in len.chars() {
                data.push(char as u8 - 48);
            }
            data.push(BINARY_PACKET_SEPARATOR_V3); // separator
            data.extend_from_slice(packet.as_bytes()); // packet
        }
    };
    Ok(())
}

/// Encode one packet into a *string* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub fn v3_string_packet_encoder(packet: Packet, data: &mut Vec<u8>) -> Result<(), Error> {
    use crate::payload::STRING_PACKET_SEPARATOR_V3;
    let packet: String = packet.try_into()?;
    let packet = format!(
        "{}{}{}",
        packet.chars().count(),
        STRING_PACKET_SEPARATOR_V3 as char,
        packet
    );
    data.extend_from_slice(packet.as_bytes());
    Ok(())
}

/// Encode multiple packet packet into a *string* payload if there is no binary packet or into a *binary* payload if there is binary packets
/// according to the [engine.io v4 protocol](https://socket.io/fr/docs/v4/engine-io-protocol/#http-long-polling-1)
#[cfg(feature = "v3")]
pub async fn v3_binary_encoder(
    mut rx: MutexGuard<'_, Receiver<Packet>>,
) -> Result<(Vec<u8>, bool), Error> {
    let mut data: Vec<u8> = Vec::new();
    let mut packet_buffer: Vec<Packet> = Vec::new();

    debug!("encoding payload with v3 binary encoder");
    // buffer all packets to find if there is binary packets
    let mut has_binary = false;
    while let Ok(packet) = rx.try_recv() {
        if packet.is_binary() {
            has_binary = true;
        }
        debug!("sending packet: {:?}", packet);
        packet_buffer.push(packet);
    }

    if has_binary {
        for packet in packet_buffer {
            v3_bin_packet_encoder(packet, &mut data)?
        }
    } else {
        for packet in packet_buffer {
            v3_string_packet_encoder(packet, &mut data)?;
        }
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packet = rx.recv().await.ok_or(Error::Aborted)?;
        debug!("sending packet: {:?}", packet);
        match packet {
            Packet::BinaryV3(_) | Packet::Binary(_) => {
                v3_bin_packet_encoder(packet, &mut data)?;
                has_binary = true;
            }
            packet => {
                v3_string_packet_encoder(packet, &mut data)?;
            }
        };
    }
    debug!("sending packet: {:?}", &data);
    Ok((data, has_binary))
}

/// Encode multiple packet packet into a *string* payload according to the
/// [engine.io v3 protocol](https://github.com/socketio/engine.io-protocol/tree/v3#payload)
#[cfg(feature = "v3")]
pub async fn v3_string_encoder(mut rx: MutexGuard<'_, Receiver<Packet>>) -> Result<Vec<u8>, Error> {
    let mut data: Vec<u8> = Vec::new();

    debug!("encoding payload with v3 string encoder");
    while let Ok(packet) = rx.try_recv() {
        v3_string_packet_encoder(packet, &mut data)?;
    }

    // If there is no packet in the buffer, wait for the next packet
    if data.is_empty() {
        let packet = rx.recv().await.ok_or(Error::Aborted)?;
        v3_string_packet_encoder(packet, &mut data)?;
    }

    Ok(data)
}

#[cfg(test)]
mod tests {

    use tokio::sync::Mutex;

    use super::*;

    #[cfg(feature = "v4")]
    #[tokio::test]
    async fn encode_v4_payload() {
        const PAYLOAD: &'static str = "4hello€\x1ebAQIDBA==\x1e4hello€";
        let (tx, rx) = tokio::sync::mpsc::channel::<Packet>(10);
        let mutex = Mutex::new(rx);
        let rx = mutex.lock().await;

        tx.try_send(Packet::Message("hello€".into())).unwrap();
        tx.try_send(Packet::Binary(vec![1, 2, 3, 4])).unwrap();
        tx.try_send(Packet::Message("hello€".into())).unwrap();
        let res = v4_encoder(rx).await.unwrap();
        assert_eq!(res, PAYLOAD.as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3b64_payload() {
        const PAYLOAD: &'static str = "7:4hello€10:b4AQIDBA==7:4hello€";
        let (tx, rx) = tokio::sync::mpsc::channel::<Packet>(10);
        let mutex = Mutex::new(rx);
        let rx = mutex.lock().await;

        tx.try_send(Packet::Message("hello€".into())).unwrap();
        tx.try_send(Packet::BinaryV3(vec![1, 2, 3, 4])).unwrap();
        tx.try_send(Packet::Message("hello€".into())).unwrap();
        let res = v3_string_encoder(rx).await.unwrap();
        assert_eq!(res, PAYLOAD.as_bytes());
    }

    #[cfg(feature = "v3")]
    #[tokio::test]
    async fn encode_v3binary_payload() {
        const PAYLOAD: [u8; 20] = [
            0, 9, 255, 52, 104, 101, 108, 108, 111, 226, 130, 172, 1, 5, 255, 4, 1, 2, 3, 4,
        ];
        let (tx, rx) = tokio::sync::mpsc::channel::<Packet>(10);
        let mutex = Mutex::new(rx);
        let rx = mutex.lock().await;

        tx.try_send(Packet::Message("hello€".into())).unwrap();
        tx.try_send(Packet::BinaryV3(vec![1, 2, 3, 4])).unwrap();
        let (res, is_binary) = v3_binary_encoder(rx).await.unwrap();
        assert_eq!(res, PAYLOAD);
        assert!(is_binary);
    }
}
