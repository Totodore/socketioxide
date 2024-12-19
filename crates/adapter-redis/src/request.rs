use serde::{Deserialize, Serialize};
use socketioxide_core::{
    adapter::{BroadcastOptions, Room},
    packet::Packet,
    Sid, Value,
};

#[derive(Debug, Deserialize)]
pub enum RequestTypeIn {
    /// Broadcast a packet to matching sockets.
    Broadcast(Packet),
    /// Broadcast a packet to matching sockets and wait for acks.
    BroadcastWithAck(Packet),
    /// Disconnect matching sockets.
    DisconnectSockets,
    /// Get all the rooms server.
    AllRooms,
    /// Add matching sockets to the rooms.
    AddSockets(Vec<Room>),
    /// Remove matching sockets from the rooms.
    DelSockets(Vec<Room>),
}

#[derive(Serialize, Debug)]
pub enum RequestTypeOut<'a> {
    /// Broadcast a packet to matching sockets.
    Broadcast(&'a Packet),
    /// Broadcast a packet to matching sockets and wait for acks.
    BroadcastWithAck(&'a Packet),
    /// Disconnect matching sockets.
    DisconnectSockets,
    /// Get all the rooms server.
    AllRooms,
    /// Add matching sockets to the rooms.
    AddSockets(&'a Vec<Room>),
    /// Remove matching sockets from the rooms.
    DelSockets(&'a Vec<Room>),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ResponseType<AckErr> {
    BroadcastAck((Sid, Result<Value, AckErr>)),
    BroadcastAckCount(u32),
    AllRooms(Vec<Room>),
}
#[derive(Serialize, Debug)]
pub struct RequestOut<'a> {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: RequestTypeOut<'a>,
    pub opts: Option<&'a BroadcastOptions>,
}
impl<'a> RequestOut<'a> {
    pub fn new(uid: Sid, r#type: RequestTypeOut<'a>, opts: &'a BroadcastOptions) -> Self {
        Self {
            uid,
            req_id: Sid::new(),
            r#type,
            opts: Some(opts),
        }
    }
    pub fn new_default_opts(uid: Sid, r#type: RequestTypeOut<'a>) -> Self {
        Self {
            uid,
            req_id: Sid::new(),
            r#type,
            opts: None,
        }
    }
}
#[derive(Deserialize, Debug)]
pub struct RequestIn {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: RequestTypeIn,
    pub opts: BroadcastOptions,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response<AckErr> {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: ResponseType<AckErr>,
}
