//! A Parser module to parse any `EngineIo` query

use std::{str::FromStr, sync::Arc};

use futures::Future;
use http::{Method, Request, Response};

use crate::{
    body::response::ResponseBody,
    config::EngineIoConfig,
    engine::EngineIo,
    handler::EngineIoHandler,
    service::futures::ResponseFuture,
    sid::Sid,
    transport::{polling, ws},
};

/// Dispatch a request according to the [`RequestInfo`] to the appropriate [`transport`](crate::transport).
pub fn dispatch_req<F, H, ReqBody, ResBody>(
    req: Request<ReqBody>,
    engine: Arc<EngineIo<H>>,
    #[cfg(feature = "hyper-v1")] hyper_v1: bool,
) -> ResponseFuture<F, ResBody>
where
    ReqBody: http_body::Body + Send + Unpin + 'static,
    ReqBody::Data: Send,
    ReqBody::Error: std::fmt::Debug,
    ResBody: Send + 'static,
    H: EngineIoHandler,
    F: Future,
{
    match RequestInfo::parse(&req, &engine.config) {
        Ok(RequestInfo {
            protocol,
            sid: None,
            transport: TransportType::Polling,
            method: Method::GET,
            #[cfg(feature = "v3")]
            b64,
        }) => ResponseFuture::ready(polling::open_req(
            engine,
            protocol,
            req,
            #[cfg(feature = "v3")]
            !b64,
        )),
        Ok(RequestInfo {
            protocol,
            sid: Some(sid),
            transport: TransportType::Polling,
            method: Method::GET,
            ..
        }) => ResponseFuture::async_response(Box::pin(polling::polling_req(engine, protocol, sid))),
        Ok(RequestInfo {
            protocol,
            sid: Some(sid),
            transport: TransportType::Polling,
            method: Method::POST,
            ..
        }) => {
            ResponseFuture::async_response(Box::pin(polling::post_req(engine, protocol, sid, req)))
        }
        Ok(RequestInfo {
            protocol,
            sid,
            transport: TransportType::Websocket,
            method: Method::GET,
            ..
        }) => ResponseFuture::ready(ws::new_req(
            engine,
            protocol,
            sid,
            req,
            #[cfg(feature = "hyper-v1")]
            hyper_v1,
        )),
        Err(e) => {
            #[cfg(feature = "tracing")]
            tracing::debug!("error parsing request: {:?}", e);
            ResponseFuture::ready(Ok(e.into()))
        }
        _req => {
            #[cfg(feature = "tracing")]
            tracing::debug!("invalid request: {:?}", _req);
            ResponseFuture::empty_response(400)
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("transport unknown")]
    UnknownTransport,
    #[error("bad handshake method")]
    BadHandshakeMethod,
    #[error("transport mismatch")]
    TransportMismatch,
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion,
}

/// Convert an error into an http response
/// If it is a known error, return the appropriate http status code
/// Otherwise, return a 500
impl<B> From<ParseError> for Response<ResponseBody<B>> {
    fn from(err: ParseError) -> Self {
        use ParseError::*;
        let conn_err_resp = |message: &'static str| {
            Response::builder()
                .status(400)
                .header("Content-Type", "application/json")
                .body(ResponseBody::custom_response(message.into()))
                .unwrap()
        };
        match err {
            UnknownTransport => conn_err_resp("{\"code\":\"0\",\"message\":\"Transport unknown\"}"),
            BadHandshakeMethod => {
                conn_err_resp("{\"code\":\"2\",\"message\":\"Bad handshake method\"}")
            }
            TransportMismatch => conn_err_resp("{\"code\":\"3\",\"message\":\"Bad request\"}"),
            UnsupportedProtocolVersion => {
                conn_err_resp("{\"code\":\"5\",\"message\":\"Unsupported protocol version\"}")
            }
        }
    }
}

/// The engine.io protocol
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    V3 = 3,
    V4 = 4,
}

impl FromStr for ProtocolVersion {
    type Err = ParseError;

    #[cfg(feature = "v3")]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(ParseError::UnsupportedProtocolVersion),
        }
    }

    #[cfg(not(feature = "v3"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(ParseError::UnsupportedProtocolVersion),
        }
    }
}

/// The type of the [`transport`](crate::transport) used by the client.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TransportType {
    Polling = 0x01,
    Websocket = 0x02,
}

impl FromStr for TransportType {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "websocket" => Ok(TransportType::Websocket),
            "polling" => Ok(TransportType::Polling),
            _ => Err(ParseError::UnknownTransport),
        }
    }
}
impl From<TransportType> for &'static str {
    fn from(t: TransportType) -> Self {
        match t {
            TransportType::Polling => "polling",
            TransportType::Websocket => "websocket",
        }
    }
}
impl From<TransportType> for String {
    fn from(t: TransportType) -> Self {
        match t {
            TransportType::Polling => "polling".into(),
            TransportType::Websocket => "websocket".into(),
        }
    }
}

/// The request information extracted from the request URI.
#[derive(Debug)]
pub struct RequestInfo {
    /// The protocol version used by the client.
    pub protocol: ProtocolVersion,
    /// The socket id if present in the request.
    pub sid: Option<Sid>,
    /// The transport type used by the client.
    pub transport: TransportType,
    /// The request method.
    pub method: Method,
    /// If the client asked for base64 encoding only.
    #[cfg(feature = "v3")]
    pub b64: bool,
}

impl RequestInfo {
    /// Parse the request URI to extract the [`TransportType`](crate::service::TransportType) and the socket id.
    fn parse<B>(req: &Request<B>, config: &EngineIoConfig) -> Result<Self, ParseError> {
        use ParseError::*;
        let query = req.uri().query().ok_or(UnknownTransport)?;

        let protocol: ProtocolVersion = query
            .split('&')
            .find(|s| s.starts_with("EIO="))
            .and_then(|s| s.split('=').nth(1))
            .ok_or(UnsupportedProtocolVersion)
            .and_then(|t| t.parse())?;

        let sid = query
            .split('&')
            .find(|s| s.starts_with("sid="))
            .and_then(|s| s.split('=').nth(1).map(|s1| s1.parse().ok()))
            .flatten();

        let transport: TransportType = query
            .split('&')
            .find(|s| s.starts_with("transport="))
            .and_then(|s| s.split('=').nth(1))
            .ok_or(UnknownTransport)
            .and_then(|t| t.parse())?;

        if !config.allowed_transport(transport) {
            return Err(TransportMismatch);
        }

        #[cfg(feature = "v3")]
        let b64: bool = query
            .split('&')
            .find(|s| s.starts_with("b64="))
            .map(|_| true)
            .unwrap_or_default();

        let method = req.method().clone();
        if !matches!(method, Method::GET) && sid.is_none() {
            Err(BadHandshakeMethod)
        } else {
            Ok(RequestInfo {
                protocol,
                sid,
                transport,
                method,
                #[cfg(feature = "v3")]
                b64,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_request(path: &str) -> Request<()> {
        Request::get(path).body(()).unwrap()
    }

    #[test]
    fn request_info_polling() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling");
        let info = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let info = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    #[cfg(feature = "v3")]
    fn request_info_polling_with_sid() {
        let req = build_request(
            "http://localhost:3000/socket.io/?EIO=3&transport=polling&sid=AAAAAAAAAAAAAAHs",
        );
        let info = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert_eq!(info.sid, Some("AAAAAAAAAAAAAAHs".parse().unwrap()));
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.protocol, ProtocolVersion::V3);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    fn request_info_websocket_with_sid() {
        let req = build_request(
            "http://localhost:3000/socket.io/?EIO=4&transport=websocket&sid=AAAAAAAAAAAAAAHs",
        );
        let info = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert_eq!(info.sid, Some("AAAAAAAAAAAAAAHs".parse().unwrap()));
        assert_eq!(info.transport, TransportType::Websocket);
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    #[cfg(feature = "v3")]
    fn request_info_polling_with_bin_by_default() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=3&transport=polling");
        let req = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert!(!req.b64);
    }

    #[test]
    #[cfg(feature = "v3")]
    fn request_info_polling_withb64() {
        assert!(cfg!(feature = "v3"));

        let req = build_request("http://localhost:3000/socket.io/?EIO=3&transport=polling&b64=1");
        let req = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert!(req.b64);
    }

    #[test]
    fn transport_unknown_err() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=grpc");
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, ParseError::UnknownTransport));
    }
    #[test]
    fn unsupported_protocol_version() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=2&transport=polling");
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, ParseError::UnsupportedProtocolVersion));
    }
    #[test]
    fn bad_handshake_method() {
        let req = Request::post("http://localhost:3000/socket.io/?EIO=4&transport=polling")
            .body(())
            .unwrap();
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, ParseError::BadHandshakeMethod));
    }

    #[test]
    fn unsupported_transport() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling");
        let err = RequestInfo::parse(
            &req,
            &EngineIoConfig::builder()
                .transports([TransportType::Websocket])
                .build(),
        )
        .unwrap_err();

        assert!(matches!(err, ParseError::TransportMismatch));

        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let err = RequestInfo::parse(
            &req,
            &EngineIoConfig::builder()
                .transports([TransportType::Polling])
                .build(),
        )
        .unwrap_err();

        assert!(matches!(err, ParseError::TransportMismatch))
    }
}
