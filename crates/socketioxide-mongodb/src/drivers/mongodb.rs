use super::{Driver, Item};
use futures_core::Stream;
use mongodb::change_stream::{event::ChangeStreamEvent, ChangeStream};

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
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project()
            .stream
            .poll_next(cx)
            .map(|result| result.map(|event| event.map(|doc| doc.full_document.unwrap())))
    }
}

impl Driver for MongoDbDriver {
    type Error = mongodb::error::Error;
    type EvStream = EvStream;

    async fn watch(&self) -> Result<EvStream, Self::Error> {
        let stream = self.collec.watch().await?;
        Ok(EvStream { stream })
    }

    async fn emit(&self, data: &Item) -> Result<(), Self::Error> {
        self.collec.insert_one(data).await?;
        Ok(())
    }
}
