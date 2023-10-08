use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use futures::SinkExt;
use http::Request;
use hyper::{body::Buf, Server};
use serde::{Deserialize, Serialize};
use socketioxide::{SocketIo, SocketIoBuilder};
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
    method: http::Method,
    body: Option<String>,
) -> String {
    let body = body
        .map(|b| hyper::Body::from(b))
        .unwrap_or_else(hyper::Body::empty);
    let req = Request::builder()
        .method(method)
        .uri(format!(
            "http://127.0.0.1:{port}/socket.io/?EIO=4&{}",
            params
        ))
        .body(body)
        .unwrap();
    let mut res = hyper::Client::new().request(req).await.unwrap();
    let body = hyper::body::aggregate(res.body_mut()).await.unwrap();
    String::from_utf8(body.chunk().to_vec())
        .unwrap()
        .chars()
        .skip(1)
        .collect()
}

pub async fn create_polling_connection(port: u16) -> String {
    let body = send_req(port, format!("transport=polling"), http::Method::GET, None).await;
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();

    send_req(
        port,
        format!("transport=polling&sid={}", open_packet.sid),
        http::Method::POST,
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

pub fn create_server(builder: SocketIoBuilder, port: u16) -> SocketIo {
    let (svc, io) = builder
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_payload(1e6 as u64)
        .build_svc();

    let addr = &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    let server = Server::bind(addr).serve(svc.into_make_service());

    tokio::spawn(server);

    io
}
