use crate::body::ResponseBody;
use crate::engine::EngineIoConfig;
use crate::packet::{OpenPacket, Packet, TransportType};
use futures_core::ready;
use http::header::{CONNECTION, SEC_WEBSOCKET_ACCEPT, UPGRADE};
use http::{HeaderValue, Response, StatusCode};
use http_body::{Body, Full};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

#[pin_project]
pub struct ResponseFuture<F, B> {
    #[pin]
    inner: ResponseFutureInner<F, B>,
}

impl<F, B> ResponseFuture<F, B> {
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
    pub fn custom_response(body: String) -> Self {
        Self {
            inner: ResponseFutureInner::CustomRespose { body },
        }
    }
    pub fn async_response(future: JoinHandle<Response<ResponseBody<B>>>) -> Self {
        Self {
            inner: ResponseFutureInner::AsyncResponse { future },
        }
    }

    pub fn new(future: F) -> Self {
        Self {
            inner: ResponseFutureInner::Future { future },
        }
    }
}
#[pin_project(project = ResFutProj)]
enum ResponseFutureInner<F, B> {
    OpenResponse {
        engine_config: EngineIoConfig,
        sid: i64,
    },
    UpgradeResponse {
        ws_key: HeaderValue,
    },
    CustomRespose {
        body: String,
    },
    EmptyResponse {
        code: u16,
    },
    AsyncResponse {
        #[pin]
        future: JoinHandle<Response<ResponseBody<B>>>,
    },
    Future {
        #[pin]
        future: F,
    },
}

impl<ResBody, F, E> Future for ResponseFuture<F, ResBody>
where
    ResBody: Body,
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match self.project().inner.project() {
            ResFutProj::Future { future } => ready!(future.poll(cx))?.map(ResponseBody::new),
            ResFutProj::OpenResponse { engine_config, sid } => {
                let body: String =
                    Packet::Open(OpenPacket::new(TransportType::Polling, *sid, engine_config))
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

            ResFutProj::CustomRespose { body } => Response::builder()
                .status(200)
                .body(ResponseBody::custom_response(Full::from(body.clone())))
                .unwrap(),
            //TODO: handle error
            ResFutProj::AsyncResponse { future } => ready!(future.poll(cx)).unwrap(),
        };
        Poll::Ready(Ok(res))
    }
}
