//! A Parser module to parse any `EngineIo` query

use std::str::FromStr;

use http::{Method, Request};

use crate::{config::EngineIoConfig, errors::Error, sid::Sid, transport::TransportType};

/// The engine.io protocol
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProtocolVersion {
    V3 = 3,
    V4 = 4,
}

impl FromStr for ProtocolVersion {
    type Err = Error;

    #[cfg(all(feature = "v3", feature = "v4"))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }

    #[cfg(all(feature = "v4", not(feature = "v3")))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4" => Ok(ProtocolVersion::V4),
            _ => Err(Error::UnsupportedProtocolVersion),
        }
    }

    #[cfg(all(feature = "v3", not(feature = "v4")))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "3" => Ok(ProtocolVersion::V3),
            _ => Err(Error::UnsupportedProtocolVersion),
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
    pub fn parse<B>(req: &Request<B>, config: &EngineIoConfig) -> Result<Self, Error> {
        let query = req.uri().query().ok_or(Error::UnknownTransport)?;

        let protocol: ProtocolVersion = query
            .split('&')
            .find(|s| s.starts_with("EIO="))
            .and_then(|s| s.split('=').nth(1))
            .ok_or(Error::UnsupportedProtocolVersion)
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
            .ok_or(Error::UnknownTransport)
            .and_then(|t| t.parse())?;

        if !config.allowed_transport(transport) {
            return Err(Error::TransportMismatch);
        }

        #[cfg(feature = "v3")]
        let b64: bool = query
            .split('&')
            .find(|s| s.starts_with("b64="))
            .map(|_| true)
            .unwrap_or_default();

        let method = req.method().clone();
        if !matches!(method, Method::GET) && sid.is_none() {
            Err(Error::BadHandshakeMethod)
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
    #[cfg(feature = "v4")]
    fn request_info_polling() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling");
        let info = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap();
        assert_eq!(info.sid, None);
        assert_eq!(info.transport, TransportType::Polling);
        assert_eq!(info.protocol, ProtocolVersion::V4);
        assert_eq!(info.method, Method::GET);
    }

    #[test]
    #[cfg(feature = "v4")]
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
    #[cfg(feature = "v4")]
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
    #[cfg(feature = "v4")]
    fn transport_unknown_err() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=grpc");
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, Error::UnknownTransport));
    }
    #[test]
    fn unsupported_protocol_version() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=2&transport=polling");
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, Error::UnsupportedProtocolVersion));
    }
    #[test]
    #[cfg(feature = "v4")]
    fn bad_handshake_method() {
        let req = Request::post("http://localhost:3000/socket.io/?EIO=4&transport=polling")
            .body(())
            .unwrap();
        let err = RequestInfo::parse(&req, &EngineIoConfig::default()).unwrap_err();
        assert!(matches!(err, Error::BadHandshakeMethod));
    }

    #[test]
    #[cfg(feature = "v4")]
    fn unsupported_transport() {
        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=polling");
        let err = RequestInfo::parse(
            &req,
            &EngineIoConfig::builder()
                .transports([TransportType::Websocket])
                .build(),
        )
        .unwrap_err();

        assert!(matches!(err, Error::TransportMismatch));

        let req = build_request("http://localhost:3000/socket.io/?EIO=4&transport=websocket");
        let err = RequestInfo::parse(
            &req,
            &EngineIoConfig::builder()
                .transports([TransportType::Polling])
                .build(),
        )
        .unwrap_err();

        assert!(matches!(err, Error::TransportMismatch))
    }
}
