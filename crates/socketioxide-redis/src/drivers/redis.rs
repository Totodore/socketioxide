use std::{
    collections::HashMap,
    fmt,
    future::Future,
    sync::{Arc, RwLock},
};

use redis::{aio::MultiplexedConnection, AsyncCommands, FromRedisValue, RedisResult};
use tokio::sync::mpsc;

use super::{Driver, MessageStream};

/// An error type for the redis driver.
#[derive(Debug)]
pub struct RedisError(redis::RedisError);

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

type HandlerMap = HashMap<String, mpsc::Sender<(String, Vec<u8>)>>;
/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
#[derive(Clone)]
pub struct RedisDriver {
    handlers: Arc<RwLock<HandlerMap>>,
    conn: MultiplexedConnection,
}

fn read_msg(msg: redis::PushInfo) -> RedisResult<Option<(String, String, Vec<u8>)>> {
    match msg.kind {
        redis::PushKind::Message => {
            if msg.data.len() < 2 {
                return Ok(None);
            }
            let mut iter = msg.data.into_iter();
            let channel: String = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            let message = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            Ok(Some((channel.clone(), channel, message)))
        }
        redis::PushKind::PMessage => {
            if msg.data.len() < 3 {
                return Ok(None);
            }
            let mut iter = msg.data.into_iter();
            let pattern = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            let channel = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            let message = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            Ok(Some((pattern, channel, message)))
        }
        _ => Ok(None),
    }
}

/// Watch for messages from the redis connection and send them to the appropriate channel
async fn watch_handler(
    mut rx: mpsc::UnboundedReceiver<redis::PushInfo>,
    handlers: Arc<RwLock<HandlerMap>>,
) {
    while let Some(info) = rx.recv().await {
        match read_msg(info) {
            Ok(Some((pattern, chan, msg))) => {
                if let Some(tx) = handlers.read().unwrap().get(&pattern) {
                    if let Err(e) = tx.try_send((chan, msg)) {
                        tracing::warn!(pattern, "redis pubsub channel full {e}");
                    }
                } else {
                    tracing::warn!(pattern, chan, "no handler for channel");
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!("error reading message from redis: {e}");
            }
        }
    }
}

impl RedisDriver {
    /// Create a new redis driver from a redis client.
    pub async fn new(client: &redis::Client) -> Result<Self, redis::RedisError> {
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

impl Driver for RedisDriver {
    type Error = RedisError;

    fn publish(
        &self,
        chan: String,
        val: Vec<u8>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let mut conn = self.conn.clone();
        async move {
            conn.publish::<_, _, redis::Value>(chan, val).await?;
            Ok(())
        }
    }

    async fn subscribe(
        &self,
        chan: String,
        size: usize,
    ) -> Result<MessageStream<(String, Vec<u8>)>, Self::Error> {
        self.conn.clone().subscribe(chan.as_str()).await?;
        let (tx, rx) = mpsc::channel(size);
        self.handlers.write().unwrap().insert(chan, tx);
        Ok(MessageStream::new(rx))
    }

    async fn psubscribe(
        &self,
        chan: String,
        size: usize,
    ) -> Result<MessageStream<(String, Vec<u8>)>, Self::Error> {
        self.conn.clone().psubscribe(chan.as_str()).await?;
        let (tx, rx) = mpsc::channel(size);
        self.handlers.write().unwrap().insert(chan, tx);
        Ok(MessageStream::new(rx))
    }

    fn unsubscribe(&self, chan: String) -> impl Future<Output = Result<(), Self::Error>> + 'static {
        self.handlers.write().unwrap().remove(&chan);
        let mut conn = self.conn.clone();
        async move {
            conn.unsubscribe(chan).await?;
            Ok(())
        }
    }

    fn punsubscribe(
        &self,
        chan: String,
    ) -> impl Future<Output = Result<(), Self::Error>> + 'static {
        self.handlers.write().unwrap().remove(&chan);
        let mut conn = self.conn.clone();
        async move {
            conn.punsubscribe(chan).await?;
            Ok(())
        }
    }

    async fn num_serv(&self, chan: &str) -> Result<u16, Self::Error> {
        let mut conn = self.conn.clone();
        let (_, count): (String, u16) = redis::cmd("PUBSUB")
            .arg("NUMSUB")
            .arg(chan)
            .query_async(&mut conn)
            .await?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use tokio::time;
    #[tokio::test]
    async fn watch_handler_message() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut handlers = HashMap::new();

        let (tx1, mut rx1) = mpsc::channel(1);
        handlers.insert("test".to_string(), tx1);
        tokio::spawn(super::watch_handler(rx, Arc::new(RwLock::new(handlers))));
        tx.send(redis::PushInfo {
            kind: redis::PushKind::Message,
            data: vec![
                redis::Value::BulkString("test".into()),
                redis::Value::BulkString("foo".into()),
            ],
        })
        .unwrap();
        let (chan, data) = time::timeout(Duration::from_millis(200), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chan, "test");
        assert_eq!(data, "foo".as_bytes());
    }

    #[tokio::test]
    async fn watch_handler_pattern() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut handlers = HashMap::new();

        let (tx1, mut rx1) = mpsc::channel(1);
        handlers.insert("test*".to_string(), tx1);
        tokio::spawn(super::watch_handler(rx, Arc::new(RwLock::new(handlers))));
        tx.send(redis::PushInfo {
            kind: redis::PushKind::PMessage,
            data: vec![
                redis::Value::BulkString("test*".into()),
                redis::Value::BulkString("test123".into()),
                redis::Value::BulkString("foo".into()),
            ],
        })
        .unwrap();
        let (chan, data) = time::timeout(Duration::from_millis(200), rx1.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chan, "test123");
        assert_eq!(data, "foo".as_bytes());
    }
}
