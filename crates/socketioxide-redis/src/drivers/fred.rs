use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

use tokio::sync::{broadcast, mpsc};

use super::{ChanItem, Driver, MessageStream};

use fred::{
    interfaces::PubsubInterface,
    prelude::{ClientLike, EventInterface, FredResult},
    types::Message,
};

pub use fred as fred_client;

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

type HandlerMap = HashMap<String, mpsc::Sender<ChanItem>>;

/// Return the channel, data and an optional req_id from a message.
fn read_msg(msg: Message) -> Option<ChanItem> {
    let chan = msg.channel.to_string();
    let data = msg.value.into_owned_bytes()?;
    Some((chan, data))
}

/// Pipe messages from the fred client to the handlers.
async fn msg_handler(mut rx: broadcast::Receiver<Message>, handlers: Arc<RwLock<HandlerMap>>) {
    loop {
        match rx.recv().await {
            Ok(msg) => {
                if let Some((chan, data)) = read_msg(msg) {
                    if let Some(tx) = handlers.read().unwrap().get(&chan) {
                        // This task is shared between all the namespaces: it must not stop
                        // because one of them does not keep up, or was closed.
                        if let Err(e) = tx.try_send((chan, data)) {
                            tracing::warn!("fred driver dropped a message: {e}");
                        }
                    } else {
                        tracing::warn!(chan, "no handler for channel");
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
        self.conn.spublish::<u16, _, _>(chan, val).await?;
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
        self.conn.sunsubscribe(chan).await?;
        Ok(())
    }

    async fn num_serv(&self, chan: &str) -> Result<u16, Self::Error> {
        let (_, num): (String, u16) = self.conn.pubsub_shardnumsub(chan).await?;
        Ok(num)
    }
}

#[cfg(test)]
mod tests {

    use fred::{
        prelude::Server,
        types::{MessageKind, Value},
    };
    use std::time::Duration;
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

    fn message(chan: &str, value: &'static [u8]) -> Message {
        Message {
            channel: chan.into(),
            kind: MessageKind::Message,
            value: Value::from_static(value),
            server: Server::new("0.0.0.0", 0),
        }
    }

    /// The dispatcher task is shared between all the namespaces: a slow namespace
    /// must not stop the delivery of the messages to the other ones.
    #[tokio::test]
    async fn full_handler_does_not_stop_dispatcher() {
        let (slow_tx, mut slow_rx) = mpsc::channel(1);
        let (other_tx, mut other_rx) = mpsc::channel(1);
        let handlers = HashMap::from([
            ("slow".to_string(), slow_tx),
            ("other".to_string(), other_tx),
        ]);
        let (tx, rx) = broadcast::channel(8);
        let task = tokio::spawn(msg_handler(rx, Arc::new(RwLock::new(handlers))));

        // The second message does not fit in the handler channel.
        tx.send(message("slow", b"first")).unwrap();
        tx.send(message("slow", b"second")).unwrap();
        tx.send(message("other", b"third")).unwrap();

        let (chan, data) = time::timeout(TIMEOUT, other_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chan, "other");
        assert_eq!(data, b"third");
        assert!(!task.is_finished());

        // Only the message that did not fit is lost.
        let (_, data) = slow_rx.try_recv().unwrap();
        assert_eq!(data, b"first");
        assert!(slow_rx.try_recv().is_err());
    }

    /// A handler can be dropped while it is still in the map.
    #[tokio::test]
    async fn closed_handler_does_not_stop_dispatcher() {
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_rx);
        let (other_tx, mut other_rx) = mpsc::channel(1);
        let handlers = HashMap::from([
            ("closed".to_string(), closed_tx),
            ("other".to_string(), other_tx),
        ]);
        let (tx, rx) = broadcast::channel(8);
        let task = tokio::spawn(msg_handler(rx, Arc::new(RwLock::new(handlers))));

        tx.send(message("closed", b"first")).unwrap();
        tx.send(message("other", b"second")).unwrap();

        let (chan, _) = time::timeout(TIMEOUT, other_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(chan, "other");
        assert!(!task.is_finished());
    }

    #[tokio::test]
    async fn watch_handler_pattern() {
        let mut handlers = HashMap::new();

        let (tx, mut rx) = mpsc::channel(1);
        handlers.insert("test-response#namespace#uid#".to_string(), tx);
        let (tx1, rx1) = broadcast::channel(1);
        tokio::spawn(msg_handler(rx1, Arc::new(RwLock::new(handlers))));
        let msg = Message {
            channel: "test-response#namespace#uid#".into(),
            kind: MessageKind::Message,
            value: Value::from_static(b"foo"),
            server: Server::new("0.0.0.0", 0),
        };
        tx1.send(msg).unwrap();
        let (chan, data) = time::timeout(TIMEOUT, rx.recv()).await.unwrap().unwrap();
        assert_eq!(chan, "test-response#namespace#uid#");
        assert_eq!(data, "foo".as_bytes());
    }
}
