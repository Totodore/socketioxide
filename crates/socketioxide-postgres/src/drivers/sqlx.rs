use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use sqlx::{
    PgPool,
    postgres::{PgListener, PgNotification},
};

use super::Driver;

pub use sqlx as sqlx_client;

/// A [`Driver`] implementation using the [`sqlx`] PostgreSQL client.
///
/// It uses [`PgListener`] for LISTEN/NOTIFY and [`PgPool`] for queries.
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

    async fn notify(&self, channel: &str, message: &str) -> Result<(), Self::Error> {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(channel)
            .bind(message)
            .execute(&self.client)
            .await?;
        Ok(())
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
