use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use serde::Serialize;
use sqlx::{
    PgPool,
    postgres::{PgListener, PgNotification},
};

use super::Driver;

pub use sqlx as sqlx_client;

#[derive(Debug, Clone)]
pub struct SqlxDriver {
    client: PgPool,
}

impl SqlxDriver {
    /// Create a new SqlxDriver instance.
    pub fn new(client: PgPool) -> Self {
        Self { client }
    }
}

impl Driver for SqlxDriver {
    type Error = sqlx::Error;
    type Notification = PgNotification;
    type NotificationStream = BoxStream<'static, Self::Notification>;

    async fn init(&self, table: &str) -> Result<(), Self::Error> {
        sqlx::query(&format!(
            r#"CREATE TABLE IF NOT EXISTS "{table}" (
                id BIGSERIAL UNIQUE,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                payload BYTEA
        )"#,
        ))
        .execute(&self.client)
        .await?;

        Ok(())
    }

    async fn listen(&self, channels: &[&str]) -> Result<Self::NotificationStream, Self::Error> {
        let mut listener = PgListener::connect_with(&self.client).await?;
        listener.listen_all(channels.iter().copied()).await?;

        let stream = listener.into_stream();
        let stream = stream.filter_map(async |res| {
            res.inspect_err(|err| {
                tracing::warn!("failed to pull sqlx notification from stream: {err}")
            })
            .ok()
        });

        Ok(Box::pin(stream))
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
            sqlx::query("SELECT pg_notify($1, $2)")
                .bind(channel)
                .bind(msg)
                .execute(&client)
                .await?;
            Ok(())
        }
    }
}

impl super::Notification for PgNotification {
    fn channel(&self) -> &str {
        PgNotification::channel(self)
    }

    fn payload(&self) -> &str {
        PgNotification::payload(self)
    }
}
