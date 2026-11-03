//! Only the configured path and its sub paths are handled by the service.
//! The other requests go to the inner service, even if they share the same prefix.

use std::sync::Arc;

use bytes::Bytes;
use engineioxide::{
    Str,
    config::EngineIoConfig,
    handler::EngineIoHandler,
    service::{EngineIoService, NotFoundService},
    socket::{DisconnectReason, Socket},
};
use http::{Request, StatusCode};
use http_body_util::Empty;
use tower_service::Service;

#[derive(Debug)]
struct MyHandler;
impl EngineIoHandler for MyHandler {
    type Data = ();
    fn on_connect(self: Arc<Self>, _: Arc<Socket<()>>) {}
    fn on_disconnect(&self, _: Arc<Socket<()>>, _: DisconnectReason) {}
    fn on_message(self: &Arc<Self>, _: Str, _: Arc<Socket<()>>) {}
    fn on_binary(self: &Arc<Self>, _: Bytes, _: Arc<Socket<()>>) {}
}

async fn status(svc: &mut EngineIoService<MyHandler, NotFoundService>, path: &str) -> StatusCode {
    let req = Request::get(format!("http://127.0.0.1{path}?EIO=4&transport=polling"))
        .body(Empty::<Bytes>::new())
        .unwrap();
    svc.call(req).await.unwrap().status()
}

fn service(req_path: &'static str) -> EngineIoService<MyHandler, NotFoundService> {
    let config = EngineIoConfig::builder().req_path(req_path).build();
    EngineIoService::with_config(Arc::new(MyHandler), config)
}

#[tokio::test]
async fn req_path_and_sub_paths_are_handled() {
    let mut svc = service("/engine.io");
    for path in ["/engine.io", "/engine.io/", "/engine.io/sub"] {
        assert_eq!(status(&mut svc, path).await, StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn paths_with_the_same_prefix_are_not_handled() {
    let mut svc = service("/engine.io");
    for path in ["/engine.io-admin", "/engine.iox/sub", "/engine.io.js"] {
        assert_eq!(
            status(&mut svc, path).await,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn unrelated_paths_are_not_handled() {
    let mut svc = service("/engine.io");
    for path in ["/", "/other", "/other/engine.io"] {
        assert_eq!(
            status(&mut svc, path).await,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn req_path_with_a_trailing_slash() {
    let mut svc = service("/engine.io/");
    assert_eq!(status(&mut svc, "/engine.io/").await, StatusCode::OK);
    assert_eq!(status(&mut svc, "/engine.io/sub").await, StatusCode::OK);
    assert_eq!(
        status(&mut svc, "/engine.io-admin").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn root_req_path() {
    let mut svc = service("/");
    assert_eq!(status(&mut svc, "/").await, StatusCode::OK);
    assert_eq!(status(&mut svc, "/anything").await, StatusCode::OK);
}
