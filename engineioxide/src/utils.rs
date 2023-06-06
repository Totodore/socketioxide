use std::sync::Mutex;

use lazy_static::lazy_static;
use snowflake::SnowflakeIdGenerator;
use tokio::sync::mpsc::{self, error::SendError};

pub fn generate_sid() -> i64 {
    lazy_static! {
        static ref ID_GENERATOR: Mutex<SnowflakeIdGenerator> =
            Mutex::new(SnowflakeIdGenerator::new(1, 1));
    }
    let id = ID_GENERATOR.lock().unwrap().real_time_generate();
    tracing::debug!("Generating new sid: {}", id);
    id
}

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

#[test]
fn test_generate_sid() {
    let id = generate_sid();
    let id2 = generate_sid();
    assert!(id != id2);
}
