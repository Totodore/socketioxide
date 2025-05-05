use std::sync::Arc;

use tokio_postgres::{Client, Connection};

use crate::PostgresAdapterConfig;

use super::Driver;

#[derive(Debug, Clone)]
pub struct PostgresDriver {
    client: Arc<Client>,
}

impl PostgresDriver {
    pub fn new<S, T>(client: Client, connection: Connection<S, T>) -> Self {
        PostgresDriver {
            client: Arc::new(client),
        }
    }
}

impl Driver for PostgresDriver {
    type Error = tokio_postgres::Error;
    async fn init(&self, table: &str, channels: &[&str]) -> Result<(), Self::Error> {
        self.client
            .execute("CREATE TABLE $1 IF NOT EXISTS", &[&table])
            .await?;
        Ok(())
    }

    async fn notify(&self, channel: &str, msg: &str) -> Result<(), Self::Error> {
        todo!()
    }
}
