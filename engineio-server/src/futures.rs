use crate::body::ResponseBody;
use crate::engine::EngineIoConfig;
use crate::packet::{OpenPacket, Packet, TransportType};
use crate::utils::generate_sid;
use futures_core::ready;
use http::header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, UPGRADE};
use http::{HeaderValue, Response, StatusCode};
use http_body::{Body, Full};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

#[pin_project]
pub struct ResponseFuture<F> {
    #[pin]
    inner: ResponseFutureInner<F>,
}

impl<F> ResponseFuture<F> {
    pub fn open_response(engine_config: EngineIoConfig, sid: i64) -> Self {
        Self {
            inner: ResponseFutureInner::OpenResponse { engine_config, sid },
        }
    }
    pub fn empty_response(code: u16) -> Self {
        Self {
            inner: ResponseFutureInner::EmptyResponse { code },
        }
    }
    pub fn upgrade_response(ws_key: HeaderValue) -> Self {
        Self {
            inner: ResponseFutureInner::UpgradeResponse { ws_key },
        }
    }
    pub fn streaming_response(body: hyper::Body) -> Self {
        Self {
            inner: ResponseFutureInner::StreamingResponse {
                body: Arc::new(Mutex::new(body)),
            },
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
		engine_config: EngineIoConfig,
        sid: i64,
	},
    UpgradeResponse {
        ws_key: HeaderValue,
    },
    EmptyResponse {
        code: u16,
    },
    StreamingResponse {
        body: Arc<Mutex<hyper::Body>>,
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
            ResFutProj::OpenResponse { engine_config, sid } => {
                let body: String = Packet::Open(OpenPacket::new(TransportType::Polling, *sid, engine_config))
                    .try_into()
                    .unwrap();
                Response::builder()
                    .status(200)
                    .body(ResponseBody::custom_response(Full::from(body)))
                    .unwrap()
            }

            ResFutProj::EmptyResponse { code } => Response::builder()
                .status(*code)
                .body(ResponseBody::empty_response())
                .unwrap(),

            ResFutProj::UpgradeResponse { ws_key } => {
                let derived = derive_accept_key(ws_key.as_bytes());
                let sec = derived.parse::<HeaderValue>().unwrap();
                Response::builder()
                    .status(StatusCode::SWITCHING_PROTOCOLS)
                    .header(UPGRADE, HeaderValue::from_static("websocket"))
                    .header(CONNECTION, HeaderValue::from_static("Upgrade"))
                    .header(SEC_WEBSOCKET_ACCEPT, sec)
                    .body(ResponseBody::empty_response())
                    .unwrap()
            }

            ResFutProj::StreamingResponse { body } => Response::builder()
                .status(200)
                .body(ResponseBody::streaming_response(body.clone()))
                .unwrap(),
        };
        Poll::Ready(Ok(res))
    }
}
