use tokio::sync::mpsc::{Receiver, error::TryRecvError};

/// Peekable receiver for polling transport
/// It is a thin wrapper around a [`Receiver`](tokio::sync::mpsc::Receiver) that allows to peek the next packet without consuming it
///
/// Its main goal is to be able to peek the next packet without consuming it to calculate the
/// packet length when using polling transport to check if it fits according to the max_payload setting
#[derive(Debug)]
pub struct PeekableReceiver<T> {
    rx: Receiver<T>,
    next: Option<T>,
}
impl<T> PeekableReceiver<T> {
    pub fn new(rx: Receiver<T>) -> Self {
        Self { rx, next: None }
    }
    pub fn peek(&mut self) -> Option<&T> {
        if self.next.is_none() {
            self.next = self.rx.try_recv().ok();
        }
        self.next.as_ref()
    }
    pub async fn recv(&mut self) -> Option<T> {
        if self.next.is_none() {
            self.rx.recv().await
        } else {
            self.next.take()
        }
    }
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        if self.next.is_none() {
            self.rx.try_recv()
        } else {
            Ok(self.next.take().unwrap())
        }
    }

    pub fn close(&mut self) {
        self.rx.close()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn peek() {
        use super::PeekableReceiver;
        use engineioxide_core::Packet;
        use tokio::sync::mpsc::channel;

        let (tx, rx) = channel(1);
        let rx = Mutex::new(PeekableReceiver::new(rx));
        let mut rx = rx.lock().await;

        assert!(rx.peek().is_none());

        tx.send(Packet::Ping).await.unwrap();
        assert_eq!(rx.peek(), Some(&Packet::Ping));
        assert_eq!(rx.recv().await, Some(Packet::Ping));
        assert!(rx.peek().is_none());

        tx.send(Packet::Pong).await.unwrap();
        assert_eq!(rx.peek(), Some(&Packet::Pong));
        assert_eq!(rx.recv().await, Some(Packet::Pong));
        assert!(rx.peek().is_none());

        tx.send(Packet::Close).await.unwrap();
        assert_eq!(rx.peek(), Some(&Packet::Close));
        assert_eq!(rx.recv().await, Some(Packet::Close));
        assert!(rx.peek().is_none());

        tx.send(Packet::Close).await.unwrap();
        assert_eq!(rx.peek(), Some(&Packet::Close));
        assert_eq!(rx.recv().await, Some(Packet::Close));
        assert!(rx.peek().is_none());
    }
}
