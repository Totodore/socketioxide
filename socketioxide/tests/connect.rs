mod fixture;
mod utils;

use fixture::create_server;
use futures::StreamExt;
use socketioxide::handler::ConnectHandler;
use tokio::sync::mpsc;

use crate::fixture::create_ws_connection;

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

    let (_, mut srx) = create_ws_connection(PORT).await.split();
    assert_ok!(srx.next().await.unwrap());
    assert_ok!(srx.next().await.unwrap());

    assert_eq!(rx.recv().await.unwrap(), 3);
    assert_eq!(rx.recv().await.unwrap(), 2);
    assert_eq!(rx.recv().await.unwrap(), 1);
    rx.try_recv().unwrap_err();
}

#[tokio::test]
pub async fn connect_middleware_error() {
    const PORT: u16 = 2421;
    use tokio_tungstenite::tungstenite::Message::*;

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

    let (_, mut srx) = create_ws_connection(PORT).await.split();
    assert_ok!(srx.next().await.unwrap());
    let p = assert_ok!(srx.next().await.unwrap());
    assert_eq!(p, Text("44{\"message\":\"MyError\"}".to_string()));
    rx.recv().await.unwrap();
    rx.recv().await.unwrap();
    rx.try_recv().unwrap_err();
}