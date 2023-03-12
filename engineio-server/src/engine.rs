use std::{sync::Arc, collections::HashMap};

use http::Request;
use hyper::upgrade::Upgraded;
use tokio::sync::Mutex;
use tokio_tungstenite::WebSocketStream;


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
	pub async fn add_ws_socket(&self, sid: String, ws: WebSocketStream<Upgraded>) {
		let mut sockets = self.ws_sockets.lock().await;
		sockets.insert(sid, ws);
	}

	pub async fn add_polling_socket(&self, sid: String, sender: hyper::body::Sender) {
		let mut sockets = self.polling_sockets.lock().await;
		sockets.insert(sid, sender);
	}

	pub async fn on_polling_req<B>(&self, req: Request<B>) {
	}
}