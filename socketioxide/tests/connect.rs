mod fixture;
mod utils;
use fixture::{create_server, socketio_client};
use futures::FutureExt;
use rust_socketio::asynchronous::ClientBuilder;
use socketioxide::handler::ConnectHandler;
use tokio::sync::mpsc;

#[tokio::test]
pub async fn connect_middleware() {
    const PORT: u16 = 2420;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<usize>(100);

    let handler = |i: usize| {
        let tx1 = tx.clone();
        move || {
            tx1.try_send(i).unwrap();
        }
    };
    io.ns("/", handler(1).with(handler(2)).with(handler(3)));

    assert_ok!(socketio_client(PORT, ()).await);

    for i in 3..=1 {
        assert_eq!(rx.recv().await.unwrap(), i);
    }
}

#[tokio::test]
pub async fn connect_middleware_error() {
    const PORT: u16 = 2421;
    let io = create_server(PORT).await;
    let (tx, mut rx) = mpsc::channel::<usize>(100);

    #[derive(Debug)]
    struct MyError;

    impl std::fmt::Display for MyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "MyError")
        }
    }

    let handler = |i: usize, e: bool| {
        let tx1 = tx.clone();
        move || {
            println!("Sending {} {}", i, e);
            tx1.try_send(i).unwrap();
            if e {
                Err(MyError)
            } else {
                Ok(())
            }
        }
    };

    io.ns(
        "/",
        handler(1, false)
            .with(handler(2, true))
            .with(handler(3, false)),
    );

    let client =
        ClientBuilder::new(format!("http://127.0.0.1:{}", PORT)).on("connect_error", |err, _| {
            async move {
                dbg!(err);
            }
            .boxed()
        });

    assert_ok!(client.connect().await);
    assert_eq!(rx.recv().await.unwrap(), 3);
}
