#[async_trait::async_trait]
pub trait PubSubClient {
    type Error: Into<super::Error>;
    async fn publish(&self, channel: &str, message: &str) -> Result<(), Self::Error>;
    async fn subscribe(&self, channel: &str) -> Result<(), Self::Error>;
    async fn unsubscribe(&self, channel: &str) -> Result<(), Self::Error>;
    async fn psubscribe(&self, pattern: &str) -> Result<(), Self::Error>;
    async fn punsubscribe(&self, pattern: &str) -> Result<(), Self::Error>;
}
