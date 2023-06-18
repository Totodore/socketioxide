use http::{Response, StatusCode};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::debug;

use crate::{body::ResponseBody, packet::Packet};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error serializing json packet: {0:?}")]
    Serialize(#[from] serde_json::Error),
    #[error("error decoding binary packet from polling request: {0:?}")]
    Base64(#[from] base64::DecodeError),
    #[error("error decoding packet: {0:?}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("io error: {0:?}")]
    Io(#[from] std::io::Error),
    #[error("bad packet received")]
    BadPacket(Packet),
    #[error("ws transport error: {0:?}")]
    WsTransport(#[from] tungstenite::Error),
    #[error("http transport error: {0:?}")]
    HttpTransport(#[from] hyper::Error),
    #[error("http error: {0:?}")]
    Http(#[from] http::Error),
    #[error("internal channel error: {0:?}")]
    SendChannel(#[from] mpsc::error::TrySendError<Packet>),
    #[error("internal channel error: {0:?}")]
    RecvChannel(#[from] mpsc::error::TryRecvError),
    #[error("heartbeat timeout")]
    HeartbeatTimeout,
    #[error("upgrade error")]
    UpgradeError,
    #[error("aborted connection")]
    Aborted,

    #[error("http error response: {0:?}")]
    HttpErrorResponse(StatusCode),

    #[error("transport unknown")]
    UnknownTransport,
    #[error("bad handshake method")]
    BadHandshakeMethod,
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion,
}

/// Convert an error into an http response
/// If it is a known error, return the appropriate http status code
/// Otherwise, return a 500
impl<B> From<Error> for Response<ResponseBody<B>> {
    fn from(err: Error) -> Self {
        let conn_err_resp = |message: &'static str| {
            Response::builder()
                .status(400)
                .body(ResponseBody::custom_response(message.into()))
                .unwrap()
        };
        match err {
            Error::HttpErrorResponse(code) => Response::builder()
                .status(code)
                .body(ResponseBody::empty_response())
                .unwrap(),
            Error::BadPacket(_) => Response::builder()
                .status(400)
                .body(ResponseBody::empty_response())
                .unwrap(),
            Error::UnknownTransport => {
                conn_err_resp("{\"code\":\"0\",\"message\":\"Transport unknown\"}")
            }
            Error::BadHandshakeMethod => {
                conn_err_resp("{\"code\":\"2\",\"message\":\"Bad handshake method\"}")
            }
            Error::UnsupportedProtocolVersion => {
                conn_err_resp("{\"code\":\"5\",\"message\":\"Unsupported protocol version\"}")
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
