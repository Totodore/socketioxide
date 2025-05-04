//! Custom request and response types for remote adapters.
//! With custom serialization/deserialization to reduce the size of the messages.
use std::collections::HashSet;

use crate::{
    Sid, Uid, Value,
    adapter::{BroadcastOptions, Room},
    packet::Packet,
};
use serde::{Deserialize, Serialize, de::SeqAccess};

/// Custom ref' output request type for remote adapters
#[derive(Debug, PartialEq)]
#[non_exhaustive]
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
    /// Fetch socket data.
    FetchSockets,
    /// Heartbeat
    Heartbeat,
    /// First heartbeat
    InitHeartbeat,
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
            Self::FetchSockets => 6,
            Self::Heartbeat => 20,
            Self::InitHeartbeat => 21,
        }
    }
}

/// Custom owned input request type for remote adapters
#[derive(Debug)]
#[non_exhaustive]
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
    /// Fetch socket data.
    FetchSockets,
    /// Heartbeat
    Heartbeat,
    /// First Heartbeat
    InitHeartbeat,
}

/// Custom ref' output request for remote adapters
#[derive(Debug, PartialEq)]
pub struct RequestOut<'a> {
    /// The id of the node sending the request.
    pub node_id: Uid,
    /// The request id.
    pub id: Sid,
    /// The request type.
    pub r#type: RequestTypeOut<'a>,
    /// The corresponding broadcast options.
    pub opts: Option<&'a BroadcastOptions>,
}

impl<'a> RequestOut<'a> {
    /// Create a new request from a node sending the request a type and options.
    pub fn new(node_id: Uid, r#type: RequestTypeOut<'a>, opts: &'a BroadcastOptions) -> Self {
        Self {
            node_id,
            id: Sid::new(),
            r#type,
            opts: Some(opts),
        }
    }
    /// Create a new empty request from a node sending the request a type.
    pub fn new_empty(node_id: Uid, r#type: RequestTypeOut<'a>) -> Self {
        Self {
            node_id,
            id: Sid::new(),
            r#type,
            opts: None,
        }
    }
}

/// Custom implementation to serialize enum variant as u8.
impl<'a> Serialize for RequestOut<'a> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Debug, Serialize)]
        struct RawRequest<'a> {
            node_id: Uid,
            id: Sid,
            r#type: u8,
            packet: Option<&'a Packet>,
            rooms: Option<&'a Vec<Room>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            opts: Option<&'a BroadcastOptions>,
        }
        let raw = RawRequest::<'a> {
            node_id: self.node_id,
            id: self.id,
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

/// Custom owned input request type for remote adapters
#[derive(Debug)]
pub struct RequestIn {
    /// The id of the node sending the request.
    pub node_id: Uid,
    /// The request id.
    pub id: Sid,
    /// The request type.
    pub r#type: RequestTypeIn,
    /// The corresponding broadcast options.
    pub opts: Option<BroadcastOptions>,
}
impl<'de> Deserialize<'de> for RequestIn {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Deserialize)]
        struct RawRequest {
            node_id: Uid,
            id: Sid,
            r#type: u8,
            packet: Option<Packet>,
            rooms: Option<Vec<Room>>,
            opts: Option<BroadcastOptions>,
        }
        let raw = RawRequest::deserialize(deserializer)?;
        let err = |field| serde::de::Error::custom(format!("missing field: {}", field));
        let r#type = match raw.r#type {
            0 => RequestTypeIn::Broadcast(raw.packet.ok_or(err("packet"))?),
            1 => RequestTypeIn::BroadcastWithAck(raw.packet.ok_or(err("packet"))?),
            2 => RequestTypeIn::DisconnectSockets,
            3 => RequestTypeIn::AllRooms,
            4 => RequestTypeIn::AddSockets(raw.rooms.ok_or(err("rooms"))?),
            5 => RequestTypeIn::DelSockets(raw.rooms.ok_or(err("rooms"))?),
            6 => RequestTypeIn::FetchSockets,
            20 => RequestTypeIn::Heartbeat,
            21 => RequestTypeIn::InitHeartbeat,
            _ => return Err(serde::de::Error::custom("invalid request type")),
        };
        Ok(Self {
            node_id: raw.node_id,
            id: raw.id,
            r#type,
            opts: raw.opts,
        })
    }
}

/// Custom response type for remote adapters
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Response<D = ()> {
    /// The node id we are answering to.
    pub node_id: Uid,
    /// The response type.
    pub r#type: ResponseType<D>,
}

/// Custom response type id for remote adapters
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ResponseTypeId {
    /// Send an acknowledgement received from a socket to the remote adapter.
    BroadcastAck = 0,
    /// Send the number of acknowledgements expected on this node to the remote adapter.
    BroadcastAckCount = 1,
    /// Get all the rooms matching the broadcast options on the server.
    AllRooms = 2,
    /// Fetch the sockets matching the broadcast options on the server.
    FetchSockets = 3,
}
impl<D> From<&ResponseType<D>> for ResponseTypeId {
    fn from(r#type: &ResponseType<D>) -> Self {
        match r#type {
            ResponseType::BroadcastAck(_) => Self::BroadcastAck,
            ResponseType::BroadcastAckCount(_) => Self::BroadcastAckCount,
            ResponseType::AllRooms(_) => Self::AllRooms,
            ResponseType::FetchSockets(_) => Self::FetchSockets,
        }
    }
}

/// Custom response type for remote adapters
#[derive(Debug, PartialEq)]
pub enum ResponseType<D = ()> {
    /// Send an acknowledgement received from a socket to the remote adapter.
    BroadcastAck((Sid, Result<Value, D>)),
    /// Send the number of acknowledgements expected on this node to the remote adapter.
    BroadcastAckCount(u32),
    /// Get all the rooms matching the broadcast options on the server.
    AllRooms(HashSet<Room>),
    /// Fetch the sockets matching the broadcast options on the server.
    FetchSockets(Vec<D>),
}
impl<D: Serialize> Serialize for ResponseType<D> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::BroadcastAck((sid, res)) => (0, (sid, res)).serialize(serializer),
            Self::BroadcastAckCount(count) => (1, count).serialize(serializer),
            Self::AllRooms(rooms) => (2, rooms).serialize(serializer),
            Self::FetchSockets(sockets) => (3, sockets).serialize(serializer),
        }
    }
}
impl<'de, D: Deserialize<'de>> Deserialize<'de> for ResponseType<D> {
    fn deserialize<DE: serde::Deserializer<'de>>(deserializer: DE) -> Result<Self, DE::Error> {
        struct TupleVisitor<D>(std::marker::PhantomData<D>);
        impl<'de, D: Deserialize<'de>> serde::de::Visitor<'de> for TupleVisitor<D> {
            type Value = ResponseType<D>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "a tuple of u8 and D")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                fn deser<'de, T: Deserialize<'de>, A: SeqAccess<'de>>(
                    seq: &mut A,
                ) -> Result<T, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(1, &""))
                }

                let el = match deser::<u8, _>(&mut seq)? {
                    0 => ResponseType::BroadcastAck(deser(&mut seq)?),
                    1 => ResponseType::BroadcastAckCount(deser(&mut seq)?),
                    2 => ResponseType::AllRooms(deser(&mut seq)?),
                    3 => ResponseType::FetchSockets(deser(&mut seq)?),
                    _ => return Err(serde::de::Error::custom("invalid response type")),
                };
                Ok(el)
            }
        }

        deserializer.deserialize_tuple(2, TupleVisitor::<D>(std::marker::PhantomData))
    }
}
impl<D> Response<D> {
    /// Extract the rooms from the response if it is a [`ResponseType::AllRooms`].
    pub fn into_rooms(self) -> Option<HashSet<Room>> {
        match self.r#type {
            ResponseType::AllRooms(rooms) => Some(rooms),
            _ => None,
        }
    }
    /// Extract the sockets from the response if it is a [`ResponseType::FetchSockets`].
    pub fn into_fetch_sockets(self) -> Option<Vec<D>> {
        match self.r#type {
            ResponseType::FetchSockets(sockets) => Some(sockets),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    impl<'a> From<&'a RequestIn> for RequestOut<'a> {
        fn from(req: &'a RequestIn) -> Self {
            Self {
                node_id: req.node_id,
                id: req.id,
                opts: req.opts.as_ref(),
                r#type: match &req.r#type {
                    RequestTypeIn::Broadcast(p) => RequestTypeOut::Broadcast(p),
                    RequestTypeIn::BroadcastWithAck(p) => RequestTypeOut::BroadcastWithAck(p),
                    RequestTypeIn::DisconnectSockets => RequestTypeOut::DisconnectSockets,
                    RequestTypeIn::AllRooms => RequestTypeOut::AllRooms,
                    RequestTypeIn::AddSockets(r) => RequestTypeOut::AddSockets(r),
                    RequestTypeIn::DelSockets(r) => RequestTypeOut::DelSockets(r),
                    RequestTypeIn::FetchSockets => RequestTypeOut::FetchSockets,
                    RequestTypeIn::Heartbeat => RequestTypeOut::Heartbeat,
                    RequestTypeIn::InitHeartbeat => RequestTypeOut::InitHeartbeat,
                },
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
        let req = RequestOut::new(Uid::new(), RequestTypeOut::Broadcast(&packet), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_broadcast_with_ack_serde() {
        let packet = Packet::event("foo", Value::Str("bar".into(), None));
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Uid::new(), RequestTypeOut::BroadcastWithAck(&packet), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_add_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let rooms = vec!["foo".into(), "bar".into()];
        let req = RequestOut::new(Uid::new(), RequestTypeOut::AddSockets(&rooms), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_del_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let rooms = vec!["foo".into(), "bar".into()];
        let req = RequestOut::new(Uid::new(), RequestTypeOut::DelSockets(&rooms), &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_disconnect_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Uid::new(), RequestTypeOut::DisconnectSockets, &opts);
        assert_request_serde(req);
    }

    #[test]
    fn request_fetch_sockets_serde() {
        let opts = BroadcastOptions::new(Sid::new());
        let req = RequestOut::new(Uid::new(), RequestTypeOut::FetchSockets, &opts);
        assert_request_serde(req);
    }

    #[test]
    fn response_serde_broadcast_ack() {
        let res = Response {
            node_id: Uid::new(),
            r#type: ResponseType::BroadcastAck((
                Sid::new(),
                Ok(Value::Bytes(Bytes::from_static(b"test"))),
            )),
        };
        let serialized = rmp_serde::to_vec(&res).unwrap();
        let deserialized: Response = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(res, deserialized);
    }
    #[test]
    fn response_serde_broadcast_ack_count() {
        let res = Response {
            node_id: Uid::new(),
            r#type: ResponseType::BroadcastAckCount(42),
        };
        let serialized = rmp_serde::to_vec(&res).unwrap();
        let deserialized: Response = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(res, deserialized);
    }

    #[test]
    fn response_serde_all_rooms() {
        let rooms = ["foo".into(), "bar".into()];
        let res = Response {
            node_id: Uid::new(),
            r#type: ResponseType::AllRooms(rooms.iter().cloned().collect()),
        };
        let serialized = rmp_serde::to_vec(&res).unwrap();
        let deserialized: Response = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(res, deserialized);
    }

    #[test]
    fn response_serde_fetch_sockets() {
        let sockets = vec![Sid::new(), Sid::new()];
        let res = Response {
            node_id: Uid::new(),
            r#type: ResponseType::FetchSockets(sockets),
        };
        let serialized = rmp_serde::to_vec(&res).unwrap();
        let deserialized: Response<Sid> = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(res, deserialized);
    }
}
