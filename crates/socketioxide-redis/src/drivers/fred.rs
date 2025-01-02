use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

use tokio::sync::{broadcast, mpsc};

use super::{Driver, MessageStream};

use fred::{
    interfaces::PubsubInterface,
    prelude::{ClientLike, EventInterface, FredResult},
    types::{Message, MessageKind},
};

/// An error type for the fred driver.
#[derive(Debug)]
pub struct FredError(fred::error::Error);

impl From<fred::error::Error> for FredError {
    fn from(e: fred::error::Error) -> Self {
        Self(e)
    }
}
impl fmt::Display for FredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for FredError {}

type HandlerMap = HashMap<String, mpsc::Sender<(String, Vec<u8>)>>;

/// Return the channel pattern, channel and data from a message.
// This redis implementation doesn't give the channel pattern.
// We have to reconstruct it from the channel.
fn read_msg(msg: Message) -> Option<(Option<String>, String, Vec<u8>)> {
    let chan = msg.channel.to_string();
    let data = msg.value.into_owned_bytes()?;
    let pattern = match msg.kind {
        MessageKind::Message => None,
        MessageKind::PMessage => Some(get_pattern_from_chan(&chan)?),
        MessageKind::SMessage => None, //TODO: this might be wrong with sharded messages
    };
    Some((pattern, chan, data))
}
/// Convert a channel to a pattern. Assuming the channel/pattern are in the following format:
/// chan: `{prefix}-response#{namespace}#{uid}#{req_id}#`
/// pattern: `{prefix}-response#{namespace}#{uid}#*`
fn get_pattern_from_chan(chan: &str) -> Option<String> {
    if chan.is_empty() || !chan.ends_with("#") {
        return None;
    }
    let pos = chan[..chan.len() - 1].rfind('#')?;
    let pattern = format!("{}*", &chan[..=pos]);
    Some(pattern)
}

/// Pipe messages from the fred client to the handlers.
async fn msg_handler(mut rx: broadcast::Receiver<Message>, handlers: Arc<RwLock<HandlerMap>>) {
    loop {
        match rx.recv().await {
            Ok(msg) => {
                if let Some((pattern, chan, data)) = read_msg(msg) {
                    let pattern = pattern.as_ref().unwrap_or(&chan);
                    if let Some(tx) = handlers.read().unwrap().get(pattern) {
                        tx.try_send((chan, data)).unwrap();
                    } else {
                        tracing::warn!(pattern, chan, "no handler for channel");
                    }
                }
            }
            // From the fred docs, even if the connection closed, the receiver will not be closed.
            // Therefore if it happens, we should just return.
            Err(broadcast::error::RecvError::Closed) => return,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("fred driver pubsub channel lagged by {}", n);
            }
        }
    }
}

/// A driver implementation for the [fred](docs.rs/fred) pub/sub backend.
#[derive(Clone)]
pub struct FredDriver {
    handlers: Arc<RwLock<HandlerMap>>,
    conn: fred::clients::SubscriberClient,
}

impl FredDriver {
    /// Create a new redis driver from a redis client.
    pub async fn new(client: fred::clients::SubscriberClient) -> FredResult<Self> {
        let handlers = Arc::new(RwLock::new(HashMap::new()));
        tokio::spawn(msg_handler(client.message_rx(), handlers.clone()));
        client.init().await?;

        Ok(Self {
            conn: client,
            handlers,
        })
    }
}

impl Driver for FredDriver {
    type Error = FredError;

    async fn publish(&self, chan: String, val: Vec<u8>) -> Result<(), Self::Error> {
        // We could use the receiver count from here. This would avoid a call to `server_cnt`.
        self.conn.publish::<u16, _, _>(chan, val).await?;
        Ok(())
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

    async fn unsubscribe(&self, chan: String) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(&chan);
        self.conn.unsubscribe(chan).await?;
        Ok(())
    }

    async fn punsubscribe(&self, chan: String) -> Result<(), Self::Error> {
        self.handlers.write().unwrap().remove(&chan);
        self.conn.punsubscribe(chan).await?;
        Ok(())
    }

    async fn num_serv(&self, chan: &str) -> Result<u16, Self::Error> {
        let (_, num): (String, u16) = self.conn.pubsub_numsub(chan).await?;
        Ok(num)
    }
}

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use fred::{prelude::Server, types::Value};
    use tokio::time;
    const TIMEOUT: Duration = Duration::from_millis(100);

    use super::*;
    #[tokio::test]
    async fn watch_handle_message() {
        let mut handlers = HashMap::new();
        let (tx, mut rx) = mpsc::channel(1);
        let (tx1, rx1) = broadcast::channel(1);
        handlers.insert("test".to_string(), tx);
        tokio::spawn(msg_handler(rx1, Arc::new(RwLock::new(handlers))));
        let msg = Message {
            channel: "test".into(),
            kind: MessageKind::Message,
            value: "foo".into(),
            server: Server::new("0.0.0.0", 0),
        };
        tx1.send(msg).unwrap();
        let (chan, data) = time::timeout(TIMEOUT, rx.recv()).await.unwrap().unwrap();
        assert_eq!(chan, "test");
        assert_eq!(data, "foo".as_bytes());
    }

    #[tokio::test]
    async fn watch_handler_pattern() {
        let mut handlers = HashMap::new();

        let (tx, mut rx) = mpsc::channel(1);
        handlers.insert("test-response#namespace#uid#*".to_string(), tx);
        let (tx1, rx1) = broadcast::channel(1);
        tokio::spawn(msg_handler(rx1, Arc::new(RwLock::new(handlers))));
        let msg = Message {
            channel: "test-response#namespace#uid#req_id#".into(),
            kind: MessageKind::PMessage,
            value: Value::from_static(b"foo"),
            server: Server::new("0.0.0.0", 0),
        };
        tx1.send(msg).unwrap();
        let (chan, data) = time::timeout(TIMEOUT, rx.recv()).await.unwrap().unwrap();
        assert_eq!(chan, "test-response#namespace#uid#req_id#");
        assert_eq!(data, "foo".as_bytes());
    }

    #[test]
    fn test_get_pattern_from_chan() {
        let chan = "test-response#namespace#uid#req_id#";
        let pattern = get_pattern_from_chan(chan).unwrap();
        assert_eq!(pattern, "test-response#namespace#uid#*");
    }
}
