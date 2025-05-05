use std::{collections::HashMap, sync::Arc};

use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::mpsc;

use super::{ChanItem, Driver};

#[derive(Debug, Clone)]
pub struct SqlxDriver {
    client: PgPool,
}
impl SqlxDriver {
    pub fn new(client: PgPool) -> Self {
        Self { client }
    }

    async fn spawn_listener(&self, mut listener: PgListener, tx: mpsc::Sender<ChanItem>) {
        while let Ok(notif) = listener
            .recv()
            .await
            .inspect_err(|e| tracing::warn!(?e, "sqlx listener error"))
        {}
    }
}

impl Driver for SqlxDriver {
    type Error = sqlx::Error;

    async fn init(&self, table: &str, channels: &[&str]) -> Result<(), Self::Error> {
        sqlx::query("CREATE TABLE $1 IF NOT EXISTS")
            .bind(&table)
            .execute(&self.client)
            .await?;
        let mut listener = PgListener::connect_with(&self.client).await?;
        listener.listen_all(channels.iter().copied()).await?;

        Ok(())
    }

    async fn notify(&self, channel: &str, msg: &str) -> Result<(), Self::Error> {
        todo!()
    }
}
