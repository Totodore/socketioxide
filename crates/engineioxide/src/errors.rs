use http::{Response, StatusCode};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::body::ResponseBody;
use crate::packet::Packet;
use engineioxide_core::Sid;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error decoding binary packet from polling request: {0:?}")]
    Base64(#[from] base64::DecodeError),
    #[error("error decoding packet: {0:?}")]
    StrUtf8(#[from] std::str::Utf8Error),
    #[error("io error: {0:?}")]
    Io(#[from] std::io::Error),
    #[error("bad packet received")]
    BadPacket(Packet),
    #[error("ws transport error: {0:?}")]
    WsTransport(#[from] Box<tungstenite::Error>),
    #[error("http error: {0:?}")]
    Http(#[from] http::Error),
    #[error("internal channel error: {0:?}")]
    SendChannel(#[from] mpsc::error::TrySendError<Packet>),
    #[error("internal channel error: {0:?}")]
    RecvChannel(#[from] mpsc::error::TryRecvError),
    #[error("heartbeat timeout")]
    HeartbeatTimeout,
    #[error("upgrade error")]
    Upgrade,
    #[error("aborted connection")]
    Aborted,

    #[error("http error response: {0:?}")]
    HttpErrorResponse(StatusCode),

    #[error("unknown session id")]
    UnknownSessionID(Sid),
    #[error("transport mismatch")]
    TransportMismatch,
    #[error("payload too large")]
    PayloadTooLarge,

    #[error("Invalid packet length")]
    InvalidPacketLength,
    #[error("Invalid packet type")]
    InvalidPacketType(Option<char>),
}

/// Convert an error into an http response
/// If it is a known error, return the appropriate http status code
/// Otherwise, return a 500
impl<B> From<Error> for Response<ResponseBody<B>> {
    fn from(err: Error) -> Self {
        let conn_err_resp = |message: &'static str| {
            Response::builder()
                .status(400)
                .header("Content-Type", "application/json")
                .body(ResponseBody::custom_response(message.into()))
                .unwrap()
        };
        match err {
            Error::HttpErrorResponse(code) => Response::builder()
                .status(code)
                .body(ResponseBody::empty_response())
                .unwrap(),
            Error::BadPacket(_) | Error::InvalidPacketLength | Error::InvalidPacketType(_) => {
                Response::builder()
                    .status(400)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
            Error::PayloadTooLarge => Response::builder()
                .status(413)
                .body(ResponseBody::empty_response())
                .unwrap(),

            Error::UnknownSessionID(_) => {
                conn_err_resp("{\"code\":\"1\",\"message\":\"Session ID unknown\"}")
            }

            Error::TransportMismatch => {
                conn_err_resp("{\"code\":\"3\",\"message\":\"Bad request\"}")
            }

            _e => {
                #[cfg(feature = "tracing")]
                tracing::debug!("uncaught error {_e:?}");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
        }
    }
}
impl From<tungstenite::Error> for Error {
    fn from(err: tungstenite::Error) -> Self {
        Error::WsTransport(Box::new(err))
    }
}
