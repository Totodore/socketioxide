use tokio::sync::oneshot;
use tower::BoxError;

#[derive(thiserror::Error, Debug)]
pub enum Error {

    #[error("error serializing json packet: {0:?}")]
    SerializeError(#[from] serde_json::Error),

    #[error("invalid packet type")]
    InvalidPacketType,

    #[error("invalid event name")]
    InvalidEventName,

    #[error("internal error: {0}")]
    InternalError(#[from] BoxError),
}

#[derive(thiserror::Error, Debug)]
pub enum AckError {
    
    #[error("error serializing/deserializing json packet: {0:?}")]
    SerdeError(#[from] serde_json::Error),

    #[error("ack receive error")]
    AckReceiveError(#[from] oneshot::error::RecvError),


    #[error("ack timeout error")]
    AckTimeoutError(#[from] tokio::time::error::Elapsed),

    #[error("internal error: {0}")]
    InternalError(#[from] Error),
}