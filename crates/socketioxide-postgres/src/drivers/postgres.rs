use std::{pin::Pin, sync::Arc};

use futures_core::{Stream, stream::BoxStream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_postgres::{AsyncMessage, Client, Config, Connection, Socket};

use super::Driver;

#[derive(Debug, Clone)]
pub struct PostgresDriver {
    client: Arc<Client>,
    config: Config,
}

impl PostgresDriver {
    pub fn new<T>(config: Config) -> Self
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        PostgresDriver { config }
    }
}

impl Driver for PostgresDriver {
    type Error = tokio_postgres::Error;
    type Notification = tokio_postgres::Notification;
    type NotificationStream = BoxStream<'static, Self::Notification>;

    async fn init(
        &self,
        table: &str,
        channels: &[&str],
    ) -> Result<Self::NotificationStream, Self::Error> {
        let st = &format!(
            r#"CREATE TABLE IF NOT EXISTS "{table}" (
                id BIGSERIAL UNIQUE,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                payload BYTEA
            )"#
        );

        self.client.execute(st, &[]).await?;

        Ok(())
    }

    async fn notify(&self, channel: &str, message: &str) -> Result<(), Self::Error> {
        self.client
            .execute("SELECT pg_notify($1, $2)", &[&channel, &message])
            .await?;
        Ok(())
    }
}

impl super::Notification for tokio_postgres::Notification {
    fn channel(&self) -> &str {
        tokio_postgres::Notification::channel(self)
    }

    fn payload(&self) -> &str {
        tokio_postgres::Notification::payload(self)
    }
}
