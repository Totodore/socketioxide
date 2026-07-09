//! Small example on how to make a middleware for message events.
use std::sync::Arc;

use socketioxide::{
    adapter::Adapter,
    extract::{Data, Extension, SocketRef},
    handler::{FromMessageParts, MessageHandler, Value},
    socket::Socket,
    SocketIo,
};

/// An extension wrapper for the example.
#[derive(Debug, Clone)]
struct Info(&'static str);

/// A middleware struct that manually implements [`MessageHandler`].
/// It stores a handler that will be the next handler to be called.
struct MessageMiddleware<H> {
    handler: H,
}
impl<H> MessageMiddleware<H> {
    pub fn new(handler: H) -> Self {
        MessageMiddleware { handler }
    }
}
impl<H, A, T> MessageHandler<A, T> for MessageMiddleware<H>
where
    H: MessageHandler<A, T>,
    A: Adapter,
    T: 'static,
{
    fn call(&self, s: Arc<Socket<A>>, mut v: Value, ack_id: Option<i64>) {
        // We set an extension on the socket.
        s.extensions.insert(Info("super test!"));

        // We parse the incoming data to print it.
        let data: Result<Data<String>, _> = Data::from_message_parts(&s, &mut v, &ack_id);
        match data {
            Ok(Data(data)) => println!("received data: {:?}", data),
            Err(err) => println!("deserialization error: {:?}", err),
        };
        // We forward the call to the inner handler
        self.handler.call(s, v, ack_id);
    }
}

async fn my_first_event_handler(
    s: SocketRef,
    Data(msg): Data<String>,
    Extension(ext): Extension<Info>,
) {
    s.emit("test", &msg).unwrap();
    assert!(matches!(ext, Info("super test!")));
}

async fn my_second_event_handler(s: SocketRef, Extension(ext): Extension<Info>) {
    println!("socket: {}, info: {:?}", s.id, ext);
    assert!(matches!(ext, Info("super test!")));
}

#[tokio::main]
async fn main() {
    let (layer, io) = SocketIo::new_layer();

    io.ns("/", async move |s: SocketRef| {
        s.on("test_1", MessageMiddleware::new(my_first_event_handler));
        s.on("test_2", MessageMiddleware::new(my_second_event_handler));
    });

    let app = axum::Router::new().layer(layer);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
