use std::task::Poll;

use super::{Driver, Item};
use futures_core::Stream;
use mongodb::change_stream::{
    event::{ChangeStreamEvent, OperationType},
    ChangeStream,
};
use socketioxide_core::Uid;

#[derive(Debug, Clone)]
pub struct MongoDbDriver {
    collec: mongodb::Collection<Item>,
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
                "fullDocument.uid": {
                  "$ne": server_id.as_str(), // ignore events from self
                },
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
