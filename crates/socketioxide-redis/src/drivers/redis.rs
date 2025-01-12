use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

use redis::{aio::MultiplexedConnection, AsyncCommands, FromRedisValue, PushInfo, RedisResult};
use tokio::sync::mpsc;

use super::{ChanItem, Driver, MessageStream};

pub use redis as redis_client;

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

type HandlerMap = HashMap<String, mpsc::Sender<ChanItem>>;
/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
#[derive(Clone)]
pub struct RedisDriver {
    handlers: Arc<RwLock<HandlerMap>>,
    conn: MultiplexedConnection,
}

/// A driver implementation for the [redis](docs.rs/redis) pub/sub backend.
#[cfg(feature = "redis-cluster")]
#[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
#[derive(Clone)]
pub struct ClusterDriver {
    handlers: Arc<RwLock<HandlerMap>>,
    conn: redis::cluster_async::ClusterConnection,
}

fn read_msg(msg: redis::PushInfo) -> RedisResult<Option<(String, Vec<u8>)>> {
    match msg.kind {
        redis::PushKind::Message | redis::PushKind::SMessage => {
            if msg.data.len() < 2 {
                return Ok(None);
            }
            let mut iter = msg.data.into_iter();
            let channel: String = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            let message = FromRedisValue::from_owned_redis_value(iter.next().unwrap())?;
            Ok(Some((channel, message)))
        }
        _ => Ok(None),
    }
}

fn handle_msg(msg: PushInfo, handlers: Arc<RwLock<HandlerMap>>) {
    match read_msg(msg) {
        Ok(Some((chan, msg))) => {
            if let Some(tx) = handlers.read().unwrap().get(&chan) {
                if let Err(e) = tx.try_send((chan, msg)) {
                    tracing::warn!("redis pubsub channel full {e}");
                }
            } else {
                tracing::warn!(chan, "no handler for channel");
            }
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!("error reading message from redis: {e}");
        }
    }
}
impl RedisDriver {
    /// Create a new redis driver from a redis client.
    pub async fn new(client: &redis::Client) -> Result<Self, redis::RedisError> {
        let handlers = Arc::new(RwLock::new(HashMap::new()));
        let handlers_clone = handlers.clone();
        let config = redis::AsyncConnectionConfig::new().set_push_sender(move |msg| {
            handle_msg(msg, handlers_clone.clone());
            Ok::<(), std::convert::Infallible>(())
        });

        let conn = client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;

        Ok(Self { conn, handlers })
    }
}

#[cfg(feature = "redis-cluster")]
impl ClusterDriver {
    /// Create a new redis driver from a redis cluster client.
    #[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
    pub async fn new(
        client_builder: redis::cluster::ClusterClientBuilder,
    ) -> Result<Self, redis::RedisError> {
        let handlers = Arc::new(RwLock::new(HashMap::new()));
        let handlers_clone = handlers.clone();
        let conn = client_builder
            .push_sender(move |msg| {
                handle_msg(msg, handlers_clone.clone());
                Ok::<(), std::convert::Infallible>(())
            })
            .build()
            .unwrap()
            .get_async_connection()
            .await?;

        Ok(Self { conn, handlers })
    }
}

impl Driver for RedisDriver {
    type Error = RedisError;

    async fn publish(&self, chan: String, val: Vec<u8>) -> Result<(), Self::Error> {
        self.conn
            .clone()
            .publish::<_, _, redis::Value>(chan, val)
            .await?;
        Ok(())
    }

    async fn subscribe(
        &self,
        chan: String,
        size: usize,
    ) -> Result<MessageStream<ChanItem>, Self::Error> {
        self.conn.clone().subscribe(chan.as_str()).await?;
        let (tx, rx) = mpsc::channel(size);
        self.handlers.write().unwrap().insert(chan, tx);
        Ok(MessageStream::new(rx))
    }

    async fn unsubscribe(&self, chan: String) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(&chan);
        self.conn.clone().unsubscribe(chan).await?;
        Ok(())
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

#[cfg_attr(docsrs, doc(cfg(feature = "redis-cluster")))]
#[cfg(feature = "redis-cluster")]
impl Driver for ClusterDriver {
    type Error = RedisError;

    async fn publish(&self, chan: String, val: Vec<u8>) -> Result<(), Self::Error> {
        self.conn
            .clone()
            .spublish::<_, _, redis::Value>(chan, val)
            .await?;
        Ok(())
    }

    async fn subscribe(
        &self,
        chan: String,
        size: usize,
    ) -> Result<MessageStream<ChanItem>, Self::Error> {
        self.conn.clone().ssubscribe(chan.as_str()).await?;
        let (tx, rx) = mpsc::channel(size);
        self.handlers.write().unwrap().insert(chan, tx);
        Ok(MessageStream::new(rx))
    }

    async fn unsubscribe(&self, chan: String) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(&chan);
        self.conn.clone().sunsubscribe(chan).await?;
        Ok(())
    }

    async fn num_serv(&self, chan: &str) -> Result<u16, Self::Error> {
        let mut conn = self.conn.clone();
        let (_, count): (String, u16) = redis::cmd("PUBSUB")
            .arg("SHARDNUMSUB")
            .arg(chan)
            .query_async(&mut conn)
            .await?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn watch_handle_message() {
        let mut handlers = HashMap::new();

        let (tx, mut rx) = mpsc::channel(1);
        handlers.insert("test".to_string(), tx);
        let msg = redis::PushInfo {
            kind: redis::PushKind::Message,
            data: vec![
                redis::Value::BulkString("test".into()),
                redis::Value::BulkString("foo".into()),
            ],
        };
        super::handle_msg(msg, Arc::new(RwLock::new(handlers)));
        let (chan, data) = rx.try_recv().unwrap();
        assert_eq!(chan, "test");
        assert_eq!(data, "foo".as_bytes());
    }

    #[test]
    fn watch_handler_pattern() {
        let mut handlers = HashMap::new();

        let (tx1, mut rx1) = mpsc::channel(1);
        handlers.insert("test-response#namespace#uid#".to_string(), tx1);
        let msg = redis::PushInfo {
            kind: redis::PushKind::Message,
            data: vec![
                redis::Value::BulkString("test-response#namespace#uid#".into()),
                redis::Value::BulkString("foo".into()),
            ],
        };
        super::handle_msg(msg, Arc::new(RwLock::new(handlers)));
        let (chan, data) = rx1.try_recv().unwrap();
        assert_eq!(chan, "test-response#namespace#uid#");
        assert_eq!(data, "foo".as_bytes());
    }
}
