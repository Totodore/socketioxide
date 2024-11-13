use std::fmt;

use redis::aio;
use socketioxide::AdapterError;

use super::SocketIoRedisDriver;

#[derive(Debug)]
struct RedisError(redis::RedisError);

impl Into<AdapterError> for RedisError {
    fn into(self) -> AdapterError {
        AdapterError::from(Box::new(self.0) as Box<dyn std::error::Error + Send>)
    }
}

impl SocketIoRedisDriver for aio::PubSub {
    type Error = RedisError;

    fn publish(&self) {}

    fn susbcribe(&self) {}
}

impl fmt::Display for RedisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for RedisError {}
