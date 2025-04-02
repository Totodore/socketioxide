use futures_core::{Future, Stream};
use serde::{Deserialize, Serialize};
use socketioxide_core::{Sid, Uid};

#[cfg(feature = "mongodb")]
mod mongodb;

pub type Item = (ItemHeader, Vec<u8>);

/// A header to identify the type of message being sent, its origin, and its target.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ItemHeader {
    Req { target: Option<Uid>, origin: Uid },
    Res { request: Sid, origin: Uid },
}
impl ItemHeader {
    pub fn get_origin(&self) -> Uid {
        match self {
            ItemHeader::Req { origin, .. } => *origin,
            ItemHeader::Res { origin, .. } => *origin,
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
