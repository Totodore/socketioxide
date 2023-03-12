use std::{collections::HashMap, sync::Arc};

use http::{Request, Response};
use hyper::upgrade::Upgraded;
use tokio::sync::Mutex;
use tokio_tungstenite::WebSocketStream;
use tungstenite::protocol::Role;

use crate::{body::ResponseBody, futures::ResponseFuture, packet::Packet};

#[derive(Debug, Clone)]
pub struct EngineIoConfig {
    pub req_path: String,
}

impl Default for EngineIoConfig {
    fn default() -> Self {
        Self {
            req_path: "/engine.io".to_string(),
        }
    }
}

type SocketMap<T> = Arc<Mutex<HashMap<String, T>>>;
/// Abstract engine implementation for Engine.IO server for http polling and websocket
#[derive(Debug, Clone)]
pub struct EngineIo {
    ws_sockets: SocketMap<WebSocketStream<Upgraded>>,
    polling_sockets: SocketMap<hyper::body::Sender>,
    config: EngineIoConfig,
}

impl EngineIo {
    pub fn from_config(config: EngineIoConfig) -> Self {
        Self {
            ws_sockets: Arc::new(Mutex::new(HashMap::new())),
            polling_sockets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }
}

impl EngineIo {
    pub fn handle_http_packet(&self, sid: String, packet: Packet) {}

    pub fn handle_ws_packet(&self, sid: String, packet: Packet) {}

    pub fn on_polling_req<B>(&self, req: Request<B>) -> ResponseFuture<Response<hyper::Body>> {
        let sid = "".into();
        let (mut sender, body) = hyper::Body::channel();
        let res = Response::builder().status(200).body(body).unwrap();
        {
            let mut sockets = self.polling_sockets.blocking_lock();
            sockets.insert(sid, sender);
        }
        ResponseFuture::new(res)
    }

    pub fn upgrade_ws_req<F, B>(&self, req: Request<B>) -> ResponseFuture<F>
    where
        B: std::marker::Send,
    {
        let headers = req.headers().clone();
        let sock_map = self.ws_sockets.clone();
        let sid = "".into();
        tokio::task::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    let ws = WebSocketStream::from_raw_socket(upgraded, Role::Server, None).await;
                    println!("upgrade success: {:?}", ws.get_config());
                    let mut sockets = self.ws_sockets.lock().await;
                    sockets.insert(sid, ws);
                }
                Err(e) => println!("upgrade error: {}", e),
            }
        });
        ResponseFuture::upgrade_response(headers)
    }
}
