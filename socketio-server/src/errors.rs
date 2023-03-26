#[derive(thiserror::Error, Debug)]
pub enum Error {

    #[error("error serializing json packet: {0:?}")]
    SerializeError(#[from] serde_json::Error),

    #[error("invalid packet type")]
    InvalidPacketType,
}