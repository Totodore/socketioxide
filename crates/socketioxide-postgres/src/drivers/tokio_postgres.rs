use std::sync::{Arc, RwLock};

use futures_util::{StreamExt, sink, stream};
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, Client, Config, Socket, tls::MakeTlsConnect};

use crate::stream::ChanStream;

use super::Driver;

pub use tokio_postgres as tokio_postgres_client;

type Listeners = Vec<(String, mpsc::Sender<tokio_postgres::Notification>)>;

const LISTENER_QUEUE_SIZE: usize = 255;

/// A [`Driver`] implementation using the [`tokio_postgres`] PostgreSQL client.
///
/// It drives the client connection to extract notifications from the PostgreSQL server.
#[derive(Debug, Clone)]
pub struct TokioPostgresDriver {
    client: Arc<Client>,
    listeners: Arc<RwLock<Listeners>>,
}

async fn dispatch_notifs(
    listeners: Arc<RwLock<Listeners>>,
    msg: AsyncMessage,
) -> Result<Arc<RwLock<Listeners>>, tokio_postgres::Error> {
    let AsyncMessage::Notification(notif) = msg else {
        return Ok(listeners);
    };

    tracing::trace!(
        chan = notif.channel(),
        "dispatching postgre notif to listeners"
    );

    if let Some((_, tx)) = listeners
        .read()
        .unwrap()
        .iter()
        .find(|(chan, _)| chan == notif.channel())
    {
        if let Err(e) = tx.try_send(notif) {
            tracing::warn!("failed to send notification: {}", e);
        }
    } else {
        tracing::debug!("no listener for channel {}", notif.channel());
    }

    Ok(listeners)
}

impl TokioPostgresDriver {
    /// Connects to the PostgreSQL server using the provided configuration and TLS settings
    /// with [`Config::connect`].
    ///
    /// The resulting connection is driven inside the driver to be
    /// able to receive notifications and dispatch them to the appropriate listeners.
    pub async fn new<T>(config: Config, tls: T) -> Result<Self, tokio_postgres::Error>
    where
        T: MakeTlsConnect<Socket> + Send + Sync + 'static,
        <T as MakeTlsConnect<Socket>>::Stream: Send,
    {
        let (client, mut conn) = config.connect(tls).await?;

        let listeners = Arc::new(RwLock::new(Vec::new()));
        let stream = stream::poll_fn(move |cx| conn.poll_message(cx));
        tokio::spawn(stream.forward(sink::unfold(listeners.clone(), dispatch_notifs)));

        let driver = TokioPostgresDriver {
            client: Arc::new(client),
            listeners,
        };

        Ok(driver)
    }
}

impl Driver for TokioPostgresDriver {
    type Error = tokio_postgres::Error;
    type Notification = tokio_postgres::Notification;
    type NotificationStream = ChanStream<Self::Notification>;

    async fn init(&self, table: &str) -> Result<(), Self::Error> {
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

    async fn listen(&self, channels: &[&str]) -> Result<Self::NotificationStream, Self::Error> {
        let (tx, rx) = mpsc::channel(LISTENER_QUEUE_SIZE);

        {
            let mut listeners = self.listeners.write().unwrap();
            for channel in channels {
                listeners.push((channel.to_string(), tx.clone()));
            }
        }

        let query: String = channels
            .iter()
            .map(|chan| format!(r#"LISTEN "{chan}";"#))
            .collect();

        self.client.batch_execute(&query).await?;

        Ok(ChanStream::new(rx))
    }

    async fn notify(&self, channel: &str, message: &str) -> Result<(), Self::Error> {
        self.client
            .execute("SELECT pg_notify($1, $2)", &[&channel, &message])
            .await?;

        Ok(())
    }

    async fn push_attachment(&self, table: &str, attachment: &[u8]) -> Result<i64, Self::Error> {
        let query = format!("INSERT INTO \"{table}\" (payload) VALUES ($1) RETURNING id");
        self.client.query_one_scalar(&query, &[&attachment]).await
    }

    async fn get_attachment(&self, table: &str, id: i64) -> Result<Vec<u8>, Self::Error> {
        let query = format!("SELECT payload FROM \"{table}\" WHERE id = $1");
        self.client.query_one_scalar(&query, &[&id]).await
    }

    async fn close(&self) -> Result<(), Self::Error> {
        self.client.execute("UNLISTEN *", &[]).await?;
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
