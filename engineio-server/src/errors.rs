use std::string::FromUtf8Error;

use tokio_tungstenite::tungstenite;

#[derive(Debug)]
pub enum Error {
	SerializeError(serde_json::Error),
	DeserializeError(serde_json::Error),
    BadPacket(&'static str),
    WsTransportError(tungstenite::Error),
    HttpTransportError(hyper::Error),
    CustomError(String),
    MultiplePollingRequests(&'static str),
    HttpBuferSyncError(&'static str),
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