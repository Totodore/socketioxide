use std::sync::Arc;

use tokio_postgres::{Client, Connection};

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
    type NotifStream<T: serde::de::DeserializeOwned + 'static>;

    async fn init(&self, table: &str, channels: &[&str]) -> Result<(), Self::Error> {
        self.client
            .execute("CREATE TABLE $1 IF NOT EXISTS", &[&table])
            .await?;

        Ok(())
    }

    fn listen<T: serde::de::DeserializeOwned + 'static>(
        &self,
        channel: &str,
    ) -> impl Future<Output = Result<Self::NotifStream<T>, Self::Error>> + Send {
        todo!()
    }

    fn notify<T: serde::Serialize + ?Sized>(
        &self,
        channel: &str,
        message: &T,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        todo!()
    }
}
