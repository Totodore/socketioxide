use std::string::FromUtf8Error;

use base64::DecodeError;
use http::{StatusCode, Response};
use tokio::sync::{mpsc};
use tokio_tungstenite::tungstenite;
use tracing::debug;

use crate::{packet::Packet, body::ResponseBody};

#[derive(Debug)]
pub enum Error {
	SerializeError(serde_json::Error),
	DeserializeError(serde_json::Error),
    Base64Error(base64::DecodeError),
    IoError(std::io::Error),
    BadPacket,
    WsTransportError(tungstenite::Error),
    HttpTransportError(hyper::Error),
    HttpError(http::Error),
    SendChannelError(mpsc::error::SendError<Packet>),
    RecvChannelError(mpsc::error::TryRecvError),
    HeartbeatTimeout,

    HttpErrorResponse(StatusCode)
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
impl From<DecodeError> for Error {
    fn from(err: DecodeError) -> Self {
        Error::Base64Error(err)
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

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err)
    }
}

/// Convert an error into an http response
/// If it is a known error, return the appropriate http status code
/// Otherwise, return a 500
impl<B> Into<Response<ResponseBody<B>>> for Error {
    fn into(self) -> Response<ResponseBody<B>> {
        match self {
            Error::HttpErrorResponse(code) => {
                Response::builder()
                    .status(code)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
            e => {
                debug!("uncaught error {e:?}");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
        }
    }
}