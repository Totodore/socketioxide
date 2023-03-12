use crate::body::ResponseBody;
use crate::packet::{OpenPacket, Packet, TransportType};
use futures_core::ready;
use http::header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use http::{HeaderMap, HeaderValue, Response, StatusCode};
use http_body::{Body, Full};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tungstenite::handshake::derive_accept_key;

#[pin_project]
pub struct ResponseFuture<F> {
    #[pin]
    inner: ResponseFutureInner<F>,
}

impl<F> ResponseFuture<F> {
    pub fn open_response(transport_type: TransportType) -> Self {
        Self {
            inner: ResponseFutureInner::OpenResponse { transport_type },
        }
    }
    pub fn empty_response(code: u16) -> Self {
        Self {
            inner: ResponseFutureInner::EmptyResponse { code },
        }
    }
    pub fn upgrade_response(req_headers: HeaderMap) -> Self {
        Self {
            inner: ResponseFutureInner::UpgradeResponse { req_headers },
        }
    }

    pub fn new(future: F) -> Self {
        Self {
            inner: ResponseFutureInner::Future { future },
        }
    }
}
#[pin_project(project = ResFutProj)]
enum ResponseFutureInner<F> {
    OpenResponse {
        transport_type: TransportType,
    },
    UpgradeResponse {
        req_headers: HeaderMap,
    },
    EmptyResponse {
        code: u16,
    },
    Future {
        #[pin]
        future: F,
    },
}

impl<ResBody, F, E> Future for ResponseFuture<F>
where
    ResBody: Body,
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match self.project().inner.project() {
            ResFutProj::Future { future } => ready!(future.poll(cx))?.map(ResponseBody::new),
            ResFutProj::OpenResponse { transport_type } => {
                //TODO: avoid cloning response
                let body: String = Packet::Open(OpenPacket::new()).try_into().unwrap();
                let code = if *transport_type == TransportType::Websocket {
                    101
                } else {
                    200
                };
                Response::builder()
                    .status(code)
                    .body(ResponseBody::custom_response(Full::from(body)))
                    .unwrap()
            }
            ResFutProj::EmptyResponse { code } => Response::builder()
                .status(*code)
                .body(ResponseBody::empty_response())
                .unwrap(),
            ResFutProj::UpgradeResponse { req_headers } => {
                let key = req_headers.get(SEC_WEBSOCKET_KEY);
                let derived = key.map(|k| derive_accept_key(k.as_bytes()));
                let sec = derived.unwrap().parse::<HeaderValue>().unwrap();
                Response::builder()
                    .status(StatusCode::SWITCHING_PROTOCOLS)
                    .header(UPGRADE, HeaderValue::from_static("websocket"))
                    .header(CONNECTION, HeaderValue::from_static("Upgrade"))
                    .header(SEC_WEBSOCKET_ACCEPT, sec)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }
        };
        Poll::Ready(Ok(res))
    }
}
