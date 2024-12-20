use serde::{Deserialize, Serialize};
use socketioxide_core::{
    adapter::{BroadcastOptions, Room},
    packet::Packet,
    Sid, Value,
};

#[derive(Debug)]
#[cfg_attr(any(test, feature = "__test_harness"), derive(PartialEq))]
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
impl RequestTypeOut<'_> {
    fn to_u8(&self) -> u8 {
        match self {
            Self::Broadcast(_) => 0,
            Self::BroadcastWithAck(_) => 1,
            Self::DisconnectSockets => 2,
            Self::AllRooms => 3,
            Self::AddSockets(_) => 4,
            Self::DelSockets(_) => 5,
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
#[cfg_attr(any(test, feature = "__test_harness"), derive(PartialEq))]
pub struct RequestOut<'a> {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: RequestTypeOut<'a>,
    pub opts: &'a BroadcastOptions,
}
impl<'a> RequestOut<'a> {
    pub fn new(uid: Sid, r#type: RequestTypeOut<'a>, opts: &'a BroadcastOptions) -> Self {
        Self {
            uid,
            req_id: Sid::new(),
            r#type,
            opts,
        }
    }
}

/// Custom implementation to serialize enum variant as u8.
impl<'a> Serialize for RequestOut<'a> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Debug, Serialize)]
        struct RawRequest<'a> {
            uid: Sid,
            req_id: Sid,
            r#type: u8,
            packet: Option<&'a Packet>,
            rooms: Option<&'a Vec<Room>>,
            opts: &'a BroadcastOptions,
        }
        let raw = RawRequest::<'a> {
            uid: self.uid,
            req_id: self.req_id,
            r#type: self.r#type.to_u8(),
            packet: match &self.r#type {
                RequestTypeOut::Broadcast(p) | RequestTypeOut::BroadcastWithAck(p) => Some(p),
                _ => None,
            },
            rooms: match &self.r#type {
                RequestTypeOut::AddSockets(r) | RequestTypeOut::DelSockets(r) => Some(r),
                _ => None,
            },
            opts: self.opts,
        };
        raw.serialize(serializer)
    }
}

#[derive(Debug)]
pub struct RequestIn {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: RequestTypeIn,
    pub opts: BroadcastOptions,
}
impl<'de> Deserialize<'de> for RequestIn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct RawRequest {
            uid: Sid,
            req_id: Sid,
            r#type: u8,
            packet: Option<Packet>,
            rooms: Option<Vec<Room>>,
            opts: BroadcastOptions,
        }
        let raw = RawRequest::deserialize(deserializer)?;
        let err = |field| serde::de::Error::custom(format!("missing field: {}", field));
        let r#type = match raw.r#type {
            0 => RequestTypeIn::Broadcast(raw.packet.ok_or(err("packet"))?),
            1 => RequestTypeIn::BroadcastWithAck(raw.packet.ok_or(err("packet"))?),
            2 => RequestTypeIn::DisconnectSockets,
            3 => RequestTypeIn::AllRooms,
            4 => RequestTypeIn::AddSockets(raw.rooms.ok_or(err("room"))?),
            5 => RequestTypeIn::DelSockets(raw.rooms.ok_or(err("room"))?),
            _ => return Err(serde::de::Error::custom("invalid request type")),
        };
        Ok(Self {
            uid: raw.uid,
            req_id: raw.req_id,
            r#type,
            opts: raw.opts,
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response<AckErr> {
    pub uid: Sid,
    pub req_id: Sid,
    pub r#type: ResponseType<AckErr>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ResponseType<AckErr> {
    BroadcastAck((Sid, Result<Value, AckErr>)),
    BroadcastAckCount(u32),
    AllRooms(Vec<Room>),
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<'a> From<&'a RequestIn> for RequestOut<'a> {
        fn from(req: &'a RequestIn) -> Self {
            Self {
                uid: req.uid,
                req_id: req.req_id,
                r#type: match &req.r#type {
                    RequestTypeIn::Broadcast(p) => RequestTypeOut::Broadcast(p),
                    RequestTypeIn::BroadcastWithAck(p) => RequestTypeOut::BroadcastWithAck(p),
                    RequestTypeIn::DisconnectSockets => RequestTypeOut::DisconnectSockets,
                    RequestTypeIn::AllRooms => RequestTypeOut::AllRooms,
                    RequestTypeIn::AddSockets(r) => RequestTypeOut::AddSockets(r),
                    RequestTypeIn::DelSockets(r) => RequestTypeOut::DelSockets(r),
                },
                opts: &req.opts,
            }
        }
    }

    fn assert_request_serde(value: RequestOut<'_>) {
        let serialized = rmp_serde::to_vec(&value).unwrap();
        let deserialized: RequestIn = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(value, (&deserialized).into())
    }

    #[test]
    fn request_broadcast_serde() {
        let packet = Packet::event("foo", Value::Str("bar".into(), None));
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Sid::new(), RequestTypeOut::Broadcast(&packet), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_broadcast_with_ack_serde() {
        let packet = Packet::event("foo", Value::Str("bar".into(), None));
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Sid::new(), RequestTypeOut::BroadcastWithAck(&packet), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_add_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let rooms = vec!["foo".into(), "bar".into()];
        let req = RequestOut::new(Sid::new(), RequestTypeOut::AddSockets(&rooms), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_del_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let rooms = vec!["foo".into(), "bar".into()];
        let req = RequestOut::new(Sid::new(), RequestTypeOut::DelSockets(&rooms), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_disconnect_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Sid::new(), RequestTypeOut::DisconnectSockets, &opts);
        assert_request_serde(req);
    }
}
