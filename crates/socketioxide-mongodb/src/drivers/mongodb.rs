use super::Driver;
use futures_util::StreamExt;
use mongodb::change_stream::{event::ChangeStreamEvent, ChangeStream};

#[derive(Debug, Clone)]
pub struct MongoDbDriver {
    collec: mongodb::Collection<Vec<u8>>,
}

impl Driver for MongoDbDriver {
    type Error = mongodb::error::Error;
    type EventStream = ChangeStream<ChangeStreamEvent<Vec<u8>>>;

    async fn watch(&self) -> Result<Self::EventStream, Self::Error> {
        // Implementation goes here
        let stream = self.collec.watch().await?;
        stream.map(|event| {
            // Process the event and return a message
            let message = format!("Event: {:?}", event);
            Ok(message)
        })
    }
}
