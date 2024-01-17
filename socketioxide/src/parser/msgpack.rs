use std::sync::Arc;
use crate::client::SocketData;
use crate::errors::Error;
use crate::packet::Packet;
use crate::parser::{Emittable, Parser};
use engineioxide::Socket as EIoSocket;

/// Alternative packet parser that is using MessagePack as its serialization type.
/// Implements the [Socket.io protocol](https://github.com/socketio/socket.io-protocol?tab=readme-ov-file). with slight modifications.
/// The main difference is that each packet is only constructed of one single binary payload, with the header encoded into the same packet as well.
#[derive(Clone, Default)]
pub struct MsgpackParser;

impl MsgpackParser {

}

impl Parser for MsgpackParser {
    fn encode<'a>(&self, packet: Packet<'a>) -> Vec<Emittable> {
        return vec![
            Emittable::Binary(vec![1,2,3,4,5,6])
        ];
    }

    fn decode_msg<'a>(&self, msg: String, socket: Arc<EIoSocket<SocketData>>) -> Result<Packet<'a>, Error> {
        println!("rec msg {:?}", msg);
        todo!()
    }

    fn decode_bin<'a>(&self, bin: Vec<u8>, socket: Arc<EIoSocket<SocketData>>) -> Option<Packet<'a>> {
        println!("rec bin {:X?}", bin);
        todo!()
    }
}