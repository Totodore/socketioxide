use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, RwLock},
};

use futures_core::Stream;
use serde::{Serialize, de::DeserializeOwned};
use sqlx::{
    PgPool,
    postgres::{PgListener, PgNotification},
};
use tokio::sync::mpsc;

use crate::PostgresAdapterConfig;

use super::Driver;
type HandlerMap = HashMap<String, mpsc::UnboundedSender<PgNotification>>;

#[derive(Debug, Clone)]
pub struct SqlxDriver {
    client: PgPool,
    handlers: Arc<RwLock<HandlerMap>>,
    config: PostgresAdapterConfig,
}
impl SqlxDriver {
    pub fn new(client: PgPool, config: PostgresAdapterConfig) -> Self {
        Self {
            client,
            handlers: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }
}

pin_project_lite::pin_project! {
    pub struct NotifStream<T> {
        #[pin]
        rx: mpsc::UnboundedReceiver<PgNotification>,
        _phantom: std::marker::PhantomData<fn() -> T>
    }
}
impl<T: DeserializeOwned> Stream for NotifStream<T> {
    type Item = T;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            std::task::Poll::Ready(_) => todo!(),
            std::task::Poll::Pending => todo!(),
        }
    }
}
impl<T> NotifStream<T> {
    pub fn new(rx: mpsc::UnboundedReceiver<PgNotification>) -> Self {
        NotifStream {
            rx,
            _phantom: PhantomData::default(),
        }
    }
}

impl Driver for SqlxDriver {
    type Error = sqlx::Error;
    type NotifStream<T: DeserializeOwned + 'static> = NotifStream<T>;
    async fn init(&self, table: &str, channels: &[&str]) -> Result<(), Self::Error> {
        sqlx::query("CREATE TABLE $1 IF NOT EXISTS")
            .bind(&table)
            .execute(&self.client)
            .await?;
        let mut listener = PgListener::connect_with(&self.client).await?;
        listener.listen_all(channels.iter().copied()).await?;
        tokio::spawn(spawn_listener(self.handlers.clone(), listener));

        Ok(())
    }
    async fn listen<T: DeserializeOwned + 'static>(
        &self,
        channel: &str,
    ) -> Result<Self::NotifStream<T>, Self::Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.handlers.write().unwrap().insert(channel.into(), tx);
        Ok(NotifStream::new(rx))
    }

    fn notify<T: Serialize + ?Sized>(
        &self,
        channel: &str,
        req: &T,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let client = self.client.clone();
        //TODO: handle error
        let msg = serde_json::to_string(req).unwrap();
        async move {
            sqlx::query("NOTIFY $1 $2")
                .bind(channel)
                .bind(msg)
                .execute(&client)
                .await?;
            Ok(())
        }
    }
}

async fn spawn_listener(handlers: Arc<RwLock<HandlerMap>>, mut listener: PgListener) {
    while let Ok(notif) = listener
        .recv()
        .await
        .inspect_err(|e| tracing::warn!(?e, "sqlx listener error"))
    {
        if let Some(tx) = handlers.read().unwrap().get(notif.channel()) {
            tx.send(notif);
        } else {
            tracing::warn!("handler not found for channel {}", notif.channel());
        }
    }
}
