use std::{
    collections::HashMap,
    fmt,
    future::Future,
    sync::{Arc, RwLock},
};

use redis::{aio::MultiplexedConnection, AsyncCommands, FromRedisValue};
use socketioxide_core::errors::AdapterError;
use tokio::sync::mpsc;

use super::{Driver, MessageStream};

/// An error type for the redis driver.
#[derive(Debug)]
pub struct RedisError(redis::RedisError);

impl Into<AdapterError> for RedisError {
    fn into(self) -> AdapterError {
        AdapterError::from(Box::new(self.0) as Box<dyn std::error::Error + Send>)
    }
}
impl From<redis::RedisError> for RedisError {
    fn from(e: redis::RedisError) -> Self {
        Self(e)
    }
}
impl fmt::Display for RedisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RedisError {}

/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
#[derive(Clone)]
pub struct RedisDriver {
    handlers: Arc<RwLock<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
    conn: MultiplexedConnection,
}

fn read_msg(msg: redis::PushInfo) -> Option<(String, Vec<u8>)> {
    match msg.kind {
        redis::PushKind::Message => {
            if msg.data.len() < 2 {
                return None;
            }
            let mut iter = msg.data.into_iter();
            let channel = FromRedisValue::from_owned_redis_value(iter.next().unwrap()).ok()?;
            let message = FromRedisValue::from_owned_redis_value(iter.next().unwrap()).ok()?;
            Some((channel, message))
        }
        _ => None,
    }
}
impl RedisDriver {
    /// Create a new redis driver from a redis client.
    pub async fn new(client: redis::Client) -> Result<Self, RedisError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let config = redis::AsyncConnectionConfig::new().set_push_sender(tx);
        let conn = client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;

        let handlers = Arc::new(RwLock::new(HashMap::new()));
        tokio::spawn(watch_handler(rx, handlers.clone()));
        Ok(Self { conn, handlers })
    }
}

/// Watch for messages from the redis connection and send them to the appropriate channel
async fn watch_handler(
    mut rx: mpsc::UnboundedReceiver<redis::PushInfo>,
    handlers: Arc<RwLock<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
) {
    while let Some(info) = rx.recv().await {
        if let Some((chan, msg)) = read_msg(info) {
            if let Some(tx) = handlers.read().unwrap().get(chan.as_str()) {
                tx.try_send(msg).ok();
            }
        }
    }
}

impl Driver for RedisDriver {
    type Error = RedisError;

    fn publish<'a>(
        &self,
        chan: &'a str,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        let mut conn = self.conn.clone();
        async move {
            conn.publish::<_, _, redis::Value>(chan, val).await?;
            Ok(())
        }
    }

    async fn subscribe(&self, chan: String) -> Result<MessageStream, Self::Error> {
        self.conn.clone().psubscribe(chan.as_str()).await?;
        let (tx, rx) = mpsc::channel(255);
        self.handlers.write().unwrap().insert(chan, tx);
        Ok(MessageStream { rx })
    }

    async fn unsubscribe(&self, chan: &str) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(chan);
        self.conn.clone().punsubscribe(chan).await?;
        Ok(())
    }

    async fn num_serv(&self, chan: &str) -> Result<u16, Self::Error> {
        let mut conn = self.conn.clone();
        let count: u16 = redis::cmd("NUMSUB")
            .arg(chan)
            .query_async(&mut conn)
            .await?;
        Ok(count)
    }
}
