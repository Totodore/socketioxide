use std::task::Poll;

use crate::MessageExpirationStrategy;

use super::{Driver, Item};
use futures_core::Stream;
use mongodb::{
    change_stream::{
        event::{ChangeStreamEvent, OperationType},
        ChangeStream,
    },
    options::IndexOptions,
    IndexModel,
};
use socketioxide_core::Uid;

pub use mongodb as mongodb_client;

#[derive(Debug, Clone)]
pub struct MongoDbDriver {
    collec: mongodb::Collection<Item>,
}

impl MongoDbDriver {
    pub async fn new(
        db: mongodb::Database,
        collection: &str,
        eviction_strategy: &MessageExpirationStrategy,
    ) -> Result<Self, mongodb::error::Error> {
        let collec = match eviction_strategy {
            MessageExpirationStrategy::CappedCollection(size) => {
                db.create_collection(collection)
                    .capped(true)
                    .size(*size)
                    .await?;
                db.collection(collection)
            }
            MessageExpirationStrategy::TtlIndex(ttl) => {
                let options = IndexOptions::builder()
                    .expire_after(*ttl)
                    .background(true)
                    .build();
                let index = IndexModel::builder()
                    .keys(mongodb::bson::doc! { "createdAt": 1 })
                    .options(options)
                    .build();

                let collec = db.collection(collection);
                collec.create_index(index).await?;
                collec
            }
        };
        Ok(Self { collec })
    }
}

pin_project_lite::pin_project! {
    pub struct EvStream {
        #[pin]
        stream: ChangeStream<ChangeStreamEvent<Item>>,
    }
}
impl Stream for EvStream {
    type Item = Result<Item, mongodb::error::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.project().stream.poll_next(cx) {
            Poll::Ready(Some(Ok(ChangeStreamEvent {
                full_document: Some(doc),
                operation_type: OperationType::Insert,
                ..
            }))) => Poll::Ready(Some(Ok(doc))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            _ => Poll::Pending,
        }
    }
}

impl Driver for MongoDbDriver {
    type Error = mongodb::error::Error;
    type EvStream = EvStream;

    async fn watch(&self, server_id: Uid) -> Result<EvStream, Self::Error> {
        let stream = self
            .collec
            .watch()
            .pipeline([mongodb::bson::doc! {
              "$match": {
                    "fullDocument.origin": { "$ne": server_id.as_str() },
                    "$or": [
                        { "fullDocument.target": server_id.as_str() },
                        { "fullDocument.target": { "$exists": false } }
                    ]
              },
            }])
            .await?;
        Ok(EvStream { stream })
    }

    async fn emit(&self, data: &Item) -> Result<(), Self::Error> {
        self.collec.insert_one(data).await?;
        Ok(())
    }
}
