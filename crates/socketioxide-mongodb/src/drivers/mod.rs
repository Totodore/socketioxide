use futures_core::{Future, Stream};
use serde::{Deserialize, Serialize};
use socketioxide_core::{Sid, Uid};

/// A driver implementation for the [mongodb](docs.rs/mongodb) change stream backend.
#[cfg(feature = "mongodb")]
pub mod mongodb;

/// The mongodb document that will be inserted in the collection to share requests/responses.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Some header infos to filter, dispatch and decode correctly requests/responses.
    #[serde(flatten)]
    pub header: ItemHeader,
    /// The targeted socket.io namespace
    pub ns: String,
    /// The origin server of the request.
    pub origin: Uid,
    /// The msgpack-encoded payload of our request/response.
    pub data: Vec<u8>,
    /// A created at flag inserted only for ttl collection and used for the ttl index.
    #[cfg(feature = "ttl-index")]
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<bson::DateTime>,
}
impl Item {
    pub(crate) fn new(
        header: ItemHeader,
        data: &impl Serialize,
        origin: Uid,
        ns: &str,
    ) -> Result<Self, rmp_serde::encode::Error> {
        let data = rmp_serde::to_vec(data)?;
        Ok(Self {
            header,
            data,
            origin,
            ns: ns.to_string(),
            created_at: None,
        })
    }
    #[cfg(feature = "ttl-index")]
    pub(crate) fn new_ttl(
        header: ItemHeader,
        data: &impl Serialize,
        origin: Uid,
        ns: &str,
    ) -> Result<Self, rmp_serde::encode::Error> {
        let data = rmp_serde::to_vec(data)?;
        Ok(Self {
            header,
            data,
            origin,
            ns: ns.to_string(),
            created_at: Some(bson::DateTime::now()),
        })
    }
}

/// A header to identify the type of message being sent, its origin, and its target.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "t")]
pub enum ItemHeader {
    /// A request.
    Req {
        /// If it is set, the request is sent to one server only.
        /// Otherwise it is considered to be a broadcast request.
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<Uid>,
    },
    /// A response.
    Res {
        /// The request ID we are answering to.
        request: Sid,
        /// The target server to send the response to. This usually corresponds to
        /// the server that sent the corresponding request.
        target: Uid,
    },
}
impl ItemHeader {
    /// Get the target of the req/res
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
    /// The event stream returned by the [`Driver::watch`] method.
    type EvStream: Stream<Item = Result<Item, Self::Error>> + Unpin + Send + 'static;

    /// Watch for document insertions on the collection. This should be implemented by change streams
    /// but could be implemented by anything else.
    ///
    /// The implementation should take care of filtering the events.
    /// It must pass events that originate from our server and if the target is set it should pass events
    /// sent for us. Here is the corresponding filter:
    /// `origin != self.uid && (target == null || target == self.uid)`.
    fn watch(
        &self,
        server_id: Uid,
        ns: &str,
    ) -> impl Future<Output = Result<Self::EvStream, Self::Error>> + Send;

    /// Emit an document to the collection.
    fn emit(&self, data: &Item) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
