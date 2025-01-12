#![allow(dead_code)]

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use engineioxide::service::NotFoundService;
use futures_util::SinkExt;
use http::Request;
use http_body_util::{BodyExt, Either, Empty, Full};
use hyper::server::conn::http1;
use hyper_util::{
    client::legacy::Client,
    rt::{TokioExecutor, TokioIo},
};

use serde::{Deserialize, Serialize};
use socketioxide::{adapter::LocalAdapter, service::SocketIoService, SocketIo};
use tokio::net::{TcpListener, TcpStream};
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
    let body = match body {
        Some(b) => Either::Left(Full::new(VecDeque::from(b.into_bytes()))),
        None => Either::Right(Empty::<VecDeque<u8>>::new()),
    };
    let req = Request::builder()
        .method(method)
        .uri(format!(
            "http://127.0.0.1:{port}/socket.io/?EIO=4&{}",
            params
        ))
        .body(body)
        .unwrap();
    let mut res = Client::builder(TokioExecutor::new())
        .build_http()
        .request(req)
        .await
        .unwrap();
    let is_json = res
        .headers()
        .get("Content-Type")
        .map(|v| v == "application/json")
        .unwrap_or_default();
    let body = res.body_mut().collect().await.unwrap().to_bytes();
    if is_json {
        String::from_utf8(body.to_vec()).unwrap()
    } else {
        String::from_utf8(body.to_vec())
            .unwrap()
            .chars()
            .skip(1)
            .collect()
    }
}

pub async fn create_polling_connection(port: u16) -> String {
    let body = send_req(
        port,
        "transport=polling".to_string(),
        http::Method::GET,
        None,
    )
    .await;
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

    ws.send(Message::Text("40{}".to_string().into()))
        .await
        .unwrap();

    ws
}

pub async fn create_server(port: u16) -> SocketIo {
    let (svc, io) = SocketIo::builder()
        .ping_interval(Duration::from_millis(300))
        .ping_timeout(Duration::from_millis(200))
        .max_buffer_size(100000)
        .build_svc();

    spawn_server(port, svc).await;
    io
}

async fn spawn_server(port: u16, svc: SocketIoService<NotFoundService, LocalAdapter>) {
    let addr = &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tokio::spawn(async move {
        // We start a loop to continuously accept incoming connections
        loop {
            let (stream, _) = listener.accept().await.unwrap();

            // Use an adapter to access something implementing `tokio::io` traits as if they implement
            // `hyper::rt` IO traits.
            let io = TokioIo::new(stream);
            let svc = svc.clone();

            // Spawn a tokio task to serve multiple connections concurrently
            tokio::task::spawn(async move {
                // Finally, we bind the incoming connection to our `hello` service
                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, svc)
                    .with_upgrades()
                    .await
                {
                    println!("Error serving connection: {:?}", err);
                }
            });
        }
    });
}
