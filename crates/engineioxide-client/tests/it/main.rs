use std::sync::Arc;

use bytes::Bytes;
use engineioxide::{DisconnectReason, Socket, Str, handler::EngineIoHandler};
use engineioxide_client::{
    Client,
    flavors::{
        hyper::HyperFlavor, hyper_tungstenite::HyperTungsteniteFlavor, testing::TestingFlavor,
    },
};

mod mock;

mod close;
mod errors;
mod handshake;
mod heartbeat;
mod payload;
mod upgrade;

const fn main() {}

#[test]
fn client_is_send() {
    fn is_send_static<T: Send + 'static>() {}
    is_send_static::<Client<HyperFlavor>>();
    is_send_static::<Client<HyperTungsteniteFlavor>>();
    is_send_static::<Client<TestingFlavor<engineioxide::service::EngineIoService<Handler>>>>();

    #[derive(Debug)]
    struct Handler;
    impl EngineIoHandler for Handler {
        type Data = ();

        fn on_connect(self: Arc<Self>, _: Arc<Socket<Self::Data>>) {}

        fn on_disconnect(&self, _: Arc<Socket<Self::Data>>, _: DisconnectReason) {}

        fn on_message(self: &Arc<Self>, _: Str, _: Arc<Socket<Self::Data>>) {}

        fn on_binary(self: &Arc<Self>, _: Bytes, _: Arc<Socket<Self::Data>>) {}
    }
}
