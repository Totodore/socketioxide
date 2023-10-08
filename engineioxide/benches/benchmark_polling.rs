use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engineioxide::socket::DisconnectReason;
use engineioxide::{handler::EngineIoHandler, service::EngineIoService, socket::Socket};

use engineioxide::sid_generator::Sid;
use http::Request;
use http_body::{Empty, Full};
use serde::{Deserialize, Serialize};
use tower::Service;

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

#[derive(Debug, Clone)]
struct Client;
impl EngineIoHandler for Client {
    type Data = ();

    fn on_connect(&self, _: Arc<Socket<Self::Data>>) {}

    fn on_disconnect(&self, _: Arc<Socket<Self::Data>>, _reason: DisconnectReason) {}

    fn on_message(&self, msg: String, socket: Arc<Socket<Self::Data>>) {
        socket.emit(msg).unwrap();
    }

    fn on_binary(&self, data: Vec<u8>, socket: Arc<Socket<Self::Data>>) {
        socket.emit_binary(data).unwrap();
    }
}

fn create_open_poll_req() -> Request<http_body::Empty<Bytes>> {
    http::Request::builder()
        .method(http::Method::GET)
        .uri("http://localhost:3000/engine.io/?EIO=4&transport=polling&t=NQ")
        .body(Empty::new())
        .unwrap()
}

async fn prepare_ping_pong(mut svc: EngineIoService<Client>) -> Sid {
    let mut res = svc.call(create_open_poll_req()).await.unwrap();
    let body = hyper::body::aggregate(res.body_mut()).await.unwrap();
    let body: String = String::from_utf8(body.chunk().to_vec())
        .unwrap()
        .chars()
        .skip(1)
        .collect();
    let open_packet: OpenPacket = serde_json::from_str(&body).unwrap();
    let sid = open_packet.sid;
    Sid::from_str(&sid).unwrap()
}
fn create_poll_req(sid: Sid) -> Request<http_body::Empty<Bytes>> {
    http::Request::builder()
        .method(http::Method::GET)
        .uri(format!(
            "http://localhost:3000/engine.io/?EIO=4&transport=polling&t=NQ&sid={sid}"
        ))
        .body(Empty::new())
        .unwrap()
}
fn create_post_req(sid: Sid) -> Request<http_body::Full<Bytes>> {
    http::Request::builder()
        .method(http::Method::POST)
        .uri(format!(
            "http://localhost:3000/engine.io/?EIO=4&transport=polling&t=NQ&sid={sid}"
        ))
        .body(Full::new("4abcabc".to_owned().into()))
        .unwrap()
}
fn create_post_bin_req(sid: Sid) -> Request<http_body::Full<Bytes>> {
    http::Request::builder()
        .method(http::Method::POST)
        .uri(format!(
            "http://localhost:3000/engine.io/?EIO=4&transport=polling&t=NQ&sid={sid}"
        ))
        .body(Full::new("bYWJjYmFj".to_owned().into()))
        .unwrap()
}
pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("polling open request", |b| {
        let svc = EngineIoService::new(Client);
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_batched(
                create_open_poll_req,
                |req| svc.clone().call(black_box(req)),
                criterion::BatchSize::SmallInput,
            );
    });

    c.bench_function("ping/pong text", |b| {
        let svc = EngineIoService::new(Client);
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_custom(|r| {
                let svc = svc.clone();
                async move {
                    futures::future::join_all((0..r).map(|_| async {
                        let sid = prepare_ping_pong(svc.clone()).await;
                        let poll_req = create_poll_req(sid);
                        let post_req = create_post_req(sid);
                        let start = std::time::Instant::now();
                        let (a, b) = futures::future::join(
                            svc.clone().call(black_box(poll_req)),
                            svc.clone().call(black_box(post_req)),
                        )
                        .await;
                        a.unwrap();
                        b.unwrap();
                        start.elapsed()
                    }))
                    .await
                    .iter()
                    .sum::<Duration>()
                        / r as u32
                }
            });
    });

    c.bench_function("ping/pong binary", |b| {
        let svc = EngineIoService::new(Client);
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter_custom(|r| {
                let svc = svc.clone();
                async move {
                    futures::future::join_all((0..r).map(|_| async {
                        let sid = prepare_ping_pong(svc.clone()).await;
                        let poll_req = create_poll_req(sid);
                        let post_req = create_post_bin_req(sid);
                        let start = std::time::Instant::now();
                        let (a, b) = futures::future::join(
                            svc.clone().call(black_box(poll_req)),
                            svc.clone().call(black_box(post_req)),
                        )
                        .await;
                        a.unwrap();
                        b.unwrap();
                        start.elapsed()
                    }))
                    .await
                    .iter()
                    .sum::<Duration>()
                        / r as u32
                }
            });
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
