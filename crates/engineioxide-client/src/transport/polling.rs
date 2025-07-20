use bytes::Bytes;
use engineioxide_core::Packet;
use http::Request;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::service::Service as HyperSvc;

pub struct PollingClient<S> {
    svc: S,
}
impl<S> PollingClient<S>
where
    S: HyperSvc<Request<Full<Bytes>>>,
    S::Response: hyper::body::Body,
    <S::Response as hyper::body::Body>::Error: std::fmt::Debug,
    <S::Response as hyper::body::Body>::Data: std::fmt::Debug,
    S::Error: std::fmt::Debug,
{
    pub fn new(svc: S) -> Self {
        Self { svc }
    }
    pub async fn handshake(&self) -> Packet {
        let req = Request::builder()
            .method("GET")
            .uri("http://localhost:3000/engine.io?EIO=4&transport=polling")
            .body(Full::default())
            .unwrap();
        let res = self.svc.call(req).await;
        let body = res.unwrap().collect().await.unwrap();
        let packet = Packet::try_from(String::from_utf8(body.to_bytes().to_vec()).unwrap());
        packet.unwrap()
    }
}
