use std::sync::Arc;

use bytes::Bytes;
use engineioxide::handler::EngineIoHandler;
use engineioxide::{DisconnectReason, service::EngineIoService};
use engineioxide::{Socket, Str};
use engineioxide_client::PollingClient;

#[derive(Debug)]
struct Handler;
impl EngineIoHandler for Handler {
    type Data = ();
    fn on_connect(self: Arc<Self>, socket: Arc<Socket<Self::Data>>) {}

    fn on_disconnect(&self, socket: Arc<Socket<Self::Data>>, reason: DisconnectReason) {}

    fn on_message(self: &Arc<Self>, msg: Str, socket: Arc<Socket<Self::Data>>) {}

    fn on_binary(self: &Arc<Self>, data: Bytes, socket: Arc<Socket<Self::Data>>) {}
}
#[tokio::test]
async fn handshake() {
    let svc = EngineIoService::new(Arc::new(Handler));
    let client = PollingClient::new(svc);
    let packet = client.handshake().await;
    dbg!(packet);
}
