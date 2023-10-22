use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use bytes::{Buf, Bytes};
use engineioxide::{config::EngineIoConfig, handler::EngineIoHandler, service::EngineIoService};
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper_util::{
    client::{connect::HttpConnector, legacy::Client},
    rt::TokioIo,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// An OpenPacket is used to initiate a connection
#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
struct OpenPacket {
    sid: String,
    upgrades: Vec<String>,
    ping_interval: u64,
    ping_timeout: u64,
    max_payload: u64,
}

/// Params should be in the form of `key1=value1&key2=value2`
pub async fn send_req(
    port: u16,
    params: String,
    method: http::Method,
    body: Option<String>,
) -> String {
    let body = body
        .map(|b| Full::new(Bytes::from(b)))
        .unwrap_or(Full::new(Bytes::new()));
    let req = hyper::Request::builder()
        .method(method)
        .uri(format!(
            "http://127.0.0.1:{port}/engine.io/?EIO=4&{}",
            params
        ))
        .body(body)
        .unwrap();

    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build(HttpConnector::new());
    let mut res = client.request(req).await.unwrap();
    let body = res.body_mut().collect().await.unwrap().to_bytes();
    String::from_utf8(body.chunk().to_vec())
        .unwrap()
        .chars()
        .skip(1)
        .collect()
}

pub async fn create_polling_connection(port: u16) -> String {
    let body = send_req(port, format!("transport=polling"), http::Method::GET, None).await;
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();
    open_packet.sid
}
pub async fn create_ws_connection(port: u16) -> WebSocketStream<MaybeTlsStream<TcpStream>> {
    tokio_tungstenite::connect_async(format!(
        "ws://127.0.0.1:{port}/engine.io/?EIO=4&transport=websocket"
    ))
    .await
    .unwrap()
    .0
}

pub fn create_server<H: EngineIoHandler>(handler: H, port: u16) {
    let config = EngineIoConfig::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build();

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    let svc = EngineIoService::with_config(handler, config);
    tokio::spawn(async move {
        // Tcp listener on addr
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        println!("Listening on: {}", addr);
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("Accepted connection");
            let io = TokioIo::new(stream);
            let svc = svc.clone();
            tokio::task::spawn(async move {
                let _ = http1::Builder::new().serve_connection(io, svc).await;
            });
        }
    });
}
