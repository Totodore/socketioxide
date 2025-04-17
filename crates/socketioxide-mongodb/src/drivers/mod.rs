use std::time::SystemTime;

use futures_core::{Future, Stream};
use serde::{Deserialize, Serialize};
use socketioxide_core::{Sid, Uid};

#[cfg(feature = "mongodb")]
pub mod mongodb;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(flatten)]
    pub header: ItemHeader,
    pub data: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<SystemTime>,
}
impl Item {
    pub(crate) fn new(
        header: ItemHeader,
        data: &impl Serialize,
    ) -> Result<Self, rmp_serde::encode::Error> {
        let data = rmp_serde::to_vec(data)?;
        Ok(Self {
            header,
            data,
            created_at: None,
        })
    }
    pub(crate) fn new_ttl(
        header: ItemHeader,
        data: &impl Serialize,
    ) -> Result<Self, rmp_serde::encode::Error> {
        let data = rmp_serde::to_vec(data)?;
        Ok(Self {
            header,
            data,
            created_at: Some(SystemTime::now()),
        })
    }
}

/// A header to identify the type of message being sent, its origin, and its target.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "t")]
pub enum ItemHeader {
    Req {
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<Uid>,
        origin: Uid,
    },
    Res {
        request: Sid,
        target: Uid,
        origin: Uid,
    },
}
impl ItemHeader {
    pub fn get_origin(&self) -> Uid {
        match self {
            ItemHeader::Req { origin, .. } => *origin,
            ItemHeader::Res { origin, .. } => *origin,
        }
    }
    pub fn get_target(&self) -> Option<Uid> {
        match self {
            ItemHeader::Req { target, .. } => *target,
            ItemHeader::Res { target, .. } => Some(*target),
        }
    }
}

/// The driver trait can be used to support different MongoDB backends.
/// It must share handlers/connection between its clones.
pub trait Driver: Clone + Send + Sync + 'static {
    /// The error type for the driver.
    type Error: std::error::Error + Send + 'static;
    type EvStream: Stream<Item = Result<Item, Self::Error>> + Unpin + Send + 'static;

    fn watch(
        &self,
        server_id: Uid,
    ) -> impl Future<Output = Result<Self::EvStream, Self::Error>> + Send;

    fn emit(&self, data: &Item) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
