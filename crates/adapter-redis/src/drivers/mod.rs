use socketioxide::AdapterError;

mod redis;

pub trait SocketIoRedisDriver: Send + Sync + 'static {
    type Error: std::error::Error + Into<AdapterError> + Send + 'static;
    fn publish(&self);
    fn susbcribe(&self);
}
