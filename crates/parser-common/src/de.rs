use std::io::Cursor;

use bytes::Buf;
use socketioxide_core::{
    packet::{Packet, PacketData},
    parser::ParseError,
    SocketIoValue, Str,
};

pub fn deserialize_packet(
    data: Str,
) -> Result<(Packet, Option<usize>), ParseError<serde_json::Error>> {
    if data.is_empty() {
        return Err(ParseError::InvalidPacketType);
    }
    // It is possible to parse the packet from a byte slice because separators are only ASCII
    let mut reader = Cursor::new(data.as_str());
    let index = reader.get_u8();
    let index = (b'0'..=b'6')
        .contains(&index)
        .then_some(index)
        .ok_or(ParseError::InvalidPacketType)?;
    dbg!(&data[reader.position() as usize..]);

    let attachments: Option<usize> = if index == b'5' || index == b'6' {
        Some(read_attachments(&mut reader).ok_or(ParseError::InvalidAttachments)?)
    } else {
        None
    };

    // Custom nsps will start with a slash
    let ns = if reader.has_remaining().then(|| reader.chunk()[0]) == Some(b'/') {
        read_nsp(&mut reader, &data)
    } else {
        Str::from("/")
    };
    dbg!(&data[reader.position() as usize..]);
    let ack = read_ack(&mut reader);
    dbg!(&data[reader.position() as usize..]);

    let data = data.slice(reader.position() as usize..);
    dbg!(&data);
    fn str(data: Str) -> SocketIoValue {
        SocketIoValue::Str((data, None))
    }
    let inner = match index {
        b'0' => PacketData::Connect((!data.is_empty()).then(|| str(data))),
        b'1' => PacketData::Disconnect,
        b'2' => PacketData::Event(str(data), ack),
        b'3' => PacketData::EventAck(str(data), ack.ok_or(ParseError::InvalidPacketType)?),
        b'5' => PacketData::BinaryEvent(str(data), ack),
        b'6' => PacketData::BinaryAck(str(data), ack.ok_or(ParseError::InvalidPacketType)?),
        _ => return Err(ParseError::InvalidPacketType),
    };
    Ok((Packet { inner, ns }, attachments))
}

fn read_attachments(reader: &mut Cursor<&str>) -> Option<usize> {
    let data = *reader.get_ref();
    let start_index = reader.position() as usize;
    loop {
        match reader.has_remaining().then(|| reader.get_u8()) {
            Some(c) if c.is_ascii_digit() => (),
            Some(b'-') if reader.position() as usize > start_index => {
                break data[start_index..reader.position() as usize - 1]
                    .parse()
                    .ok()
            }
            _ => {
                reader.set_position(reader.position() - 1);
                break None;
            }
        }
    }
}

fn read_nsp(reader: &mut Cursor<&str>, data: &Str) -> Str {
    let start_index = reader.position() as usize;
    loop {
        match reader.has_remaining().then(|| reader.get_u8()) {
            Some(b',') => {
                break data.slice(start_index..reader.position() as usize - 1);
            }
            // It maybe possible depending on clients that ns does not end with a comma
            // if it is the end of the packet
            // e.g `1/custom`
            None => {
                break data.slice(start_index..reader.position() as usize);
            }
            Some(_) => (),
        }
    }
}

fn read_ack(reader: &mut Cursor<&str>) -> Option<i64> {
    let start_index = reader.position() as usize;
    let data = *reader.get_ref();
    loop {
        match reader.has_remaining().then(|| reader.chunk()[0]) {
            Some(c) if c.is_ascii_digit() => reader.advance(1),
            Some(b'[' | b'{') if reader.position() as usize > start_index => {
                break data[start_index..reader.position() as usize].parse().ok();
            }
            _ => break None,
        }
    }
}
