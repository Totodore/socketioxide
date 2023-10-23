use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use axum::body::Bytes;
use futures::SinkExt;
use http_body_util::{BodyExt, Full};
use hyper::{body::Buf, server::conn::http1, Request};
use hyper_util::{
    client::{connect::HttpConnector, legacy::Client},
    rt::TokioIo,
};
use serde::{Deserialize, Serialize};
use socketioxide::SocketIo;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
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
    method: hyper::Method,
    body: Option<String>,
) -> String {
    let body = body
        .map(|b| Full::new(Bytes::from(b)))
        .unwrap_or(Full::new(Bytes::new()));
    let req = Request::builder()
        .method(method)
        .uri(format!(
            "http://127.0.0.1:{port}/socket.io/?EIO=4&{}",
            params
        ))
        .body(body)
        .unwrap();

    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build(HttpConnector::new());
    let mut res = client.request(req).await.unwrap();
    let is_json = res
        .headers()
        .get("Content-Type")
        .map(|v| v == "application/json")
        .unwrap_or_default();
    let body = res.body_mut().collect().await.unwrap().to_bytes();
    if is_json {
        String::from_utf8(body.chunk().to_vec()).unwrap()
    } else {
        String::from_utf8(body.chunk().to_vec())
            .unwrap()
            .chars()
            .skip(1)
            .collect()
    }
}

pub async fn create_polling_connection(port: u16) -> String {
    let body = send_req(port, format!("transport=polling"), hyper::Method::GET, None).await;
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();

    send_req(
        port,
        format!("transport=polling&sid={}", open_packet.sid),
        hyper::Method::POST,
        Some("40{}".to_string()),
    )
    .await;

    open_packet.sid
}
pub async fn create_ws_connection(port: u16) -> WebSocketStream<MaybeTlsStream<TcpStream>> {
    let mut ws = tokio_tungstenite::connect_async(format!(
        "ws://127.0.0.1:{port}/socket.io/?EIO=4&transport=websocket"
    ))
    .await
    .unwrap()
    .0;

    ws.send(Message::Text("40{}".to_string())).await.unwrap();

    ws
}

pub async fn create_server(port: u16) -> SocketIo {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    println!("Listening on: {}", addr);
    let (server, io) = SocketIo::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .build_server(addr)
        .await;

    tokio::spawn(server);
    io
}
