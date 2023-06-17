use tokio::sync::mpsc::{self, error::SendError};

/// Forwards items from a channel to another channel, applying a map function to each item
///
/// Returns an error if the sending channel is full
pub async fn forward_map_chan<T, R>(
    mut from: mpsc::Receiver<T>,
    to: mpsc::Sender<R>,
    map: impl Fn(T) -> R,
) -> Result<(), SendError<R>> {
    while let Some(item) = from.recv().await {
        let item = map(item);
        to.send(item).await?;
    }
    Ok(())
}
