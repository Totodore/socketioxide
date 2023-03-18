use std::string::FromUtf8Error;

use tokio::sync::{oneshot::error::RecvError, mpsc};
use tokio_tungstenite::tungstenite;

use crate::packet::Packet;

#[derive(Debug)]
pub enum Error {
	SerializeError(serde_json::Error),
	DeserializeError(serde_json::Error),
    BadPacket(&'static str),
    BadTransport(),
    WsTransportError(tungstenite::Error),
    HttpTransportError(hyper::Error),
    CustomError(String),
    MultiplePollingRequests(),
    HttpBufferSendError(),
    HttpBufferRecvError(RecvError),
    HttpError(http::Error),
    SendChannelError(mpsc::error::SendError<Packet>),
    RecvChannelError(mpsc::error::TryRecvError)
}

impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Self {
		Error::SerializeError(err)
	}
}
impl From<tungstenite::Error> for Error {
    fn from(err: tungstenite::Error) -> Self {
        Error::WsTransportError(err)
    }
}
impl From<hyper::Error> for Error {
    fn from(err: hyper::Error) -> Self {
        Error::HttpTransportError(err)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(err: FromUtf8Error) -> Self {
        use serde::de::Error;
        Self::DeserializeError(serde_json::Error::custom(err))
    }
}
impl From<RecvError> for Error {
    fn from(err: RecvError) -> Self {
        Error::HttpBufferRecvError(err)
    }
}
impl From<mpsc::error::SendError<Packet>> for Error {
    fn from(err: mpsc::error::SendError<Packet>) -> Self {
        Error::SendChannelError(err)
    }
}

impl From<http::Error> for Error {
    fn from(err: http::Error) -> Self {
        Error::HttpError(err)
    }
}