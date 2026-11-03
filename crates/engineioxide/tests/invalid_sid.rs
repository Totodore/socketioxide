//! A request with a malformed session id is a request for an unknown
//! session: it must be rejected, and not be handled as a new connection.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::EngineIoService,
    socket::{DisconnectReason, Socket},
};

mod fixture;
use fixture::send_req;

#[derive(Debug, Default)]
struct MyHandler {
    connected: AtomicUsize,
}

impl EngineIoHandler for MyHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, _: Arc<Socket<()>>) {
        self.connected.fetch_add(1, Ordering::SeqCst);
    }
    fn on_disconnect(&self, _: Arc<Socket<()>>, _: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, _: Str, _: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _: Bytes, _: Arc<Socket<()>>) {}
}

#[tokio::test]
async fn polling_with_a_malformed_sid_does_not_create_a_session() {
    let handler = Arc::new(MyHandler::default());
    let mut svc = EngineIoService::with_config(handler.clone(), EngineIoConfig::default());

    let params = "transport=polling&sid=garbage".to_string();
    let body = send_req(&mut svc, params, http::Method::GET, None).await;

    assert!(body.contains("Session ID unknown"), "{body}");
    assert_eq!(handler.connected.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn polling_without_sid_creates_a_session() {
    let handler = Arc::new(MyHandler::default());
    let mut svc = EngineIoService::with_config(handler.clone(), EngineIoConfig::default());

    send_req(
        &mut svc,
        "transport=polling".into(),
        http::Method::GET,
        None,
    )
    .await;

    assert_eq!(handler.connected.load(Ordering::SeqCst), 1);
}
