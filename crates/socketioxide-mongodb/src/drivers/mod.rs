use futures_core::{Future, Stream};
use socketioxide_core::{Sid, Uid};

#[cfg(feature = "mongodb")]
mod mongodb;

pub type Item = (ItemId, Vec<u8>);

/// An identifier to check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemId {
    Req(Option<Uid>),
    Res(Sid),
}
impl serde::Serialize for ItemId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Debug, serde::Serialize)]
        struct RawItemId {
            id: u8,
            sid: Option<Sid>,
            uid: Option<Uid>,
        }
        let raw = match self {
            ItemId::Req(uid) => RawItemId {
                id: 0,
                sid: None,
                uid: *uid,
            },
            ItemId::Res(sid) => RawItemId {
                id: 1,
                sid: Some(*sid),
                uid: None,
            },
        };
        raw.serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for ItemId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, serde::Deserialize)]
        struct RawItemId {
            id: u8,
            sid: Option<Sid>,
            uid: Option<Uid>,
        }
        let raw = RawItemId::deserialize(deserializer)?;
        match raw.id {
            0 => Ok(ItemId::Req(raw.uid)),
            1 if raw.sid.is_some() => Ok(ItemId::Res(raw.sid.unwrap())),
            _ => Err(serde::de::Error::custom("invalid item id")),
        }
    }
}

/// The driver trait can be used to support different MongoDB backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    /// The error type for the driver.
    type Error: std::error::Error + Send + 'static;
    type EvStream: Stream<Item = Result<Item, Self::Error>> + Unpin + Send + 'static;

    fn watch(&self) -> impl Future<Output = Result<Self::EvStream, Self::Error>> + Send;

    fn emit(&self, data: &Item) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
