mod utils;

use bytes::Bytes;
use engineioxide::Packet::*;
use serde_json::Value;
use socketioxide::{extract::SocketRef, handler::ConnectHandler, SendError, SocketError, SocketIo};
use tokio::sync::mpsc;

#[tokio::test]
pub async fn connect_middleware() {
    let (_svc, io) = SocketIo::new_svc();
    let (tx, mut rx) = mpsc::channel::<usize>(100);

    let handler = |i: usize| {
        let tx1 = tx.clone();
        move |s: SocketRef| {
            // Socket should be closed for all emit methods on it

            assert!(matches!(
                s.emit("test", ()),
                Err(SendError::Socket(SocketError::Closed(Value::Null)))
            ));

            assert!(matches!(
                s.emit_with_ack::<(), ()>("test", ()),
                Err(SendError::Socket(SocketError::Closed(())))
            ));

            assert!(matches!(
                s.bin(vec![Bytes::from_static(&[0, 1, 2, 3])])
                    .emit("test", Value::Null),
                Err(SendError::Socket(SocketError::Closed(Value::Null)))
            ));
            assert!(matches!(
                s.bin(vec![Bytes::from_static(&[0, 1, 2, 3])])
                    .emit_with_ack::<()>("test", Value::Null),
                Err(SendError::Socket(SocketError::Closed(Value::Null)))
            ));

            tx1.try_send(i).unwrap();
            Ok::<_, std::convert::Infallible>(())
        }
    };
    io.ns(
        "/",
        { || {} }.with(handler(3)).with(handler(2)).with(handler(1)),
    );

    let (_, mut srx) = io.new_dummy_sock("/", ()).await;
    assert_eq!(rx.recv().await.unwrap(), 1);
    assert_eq!(rx.recv().await.unwrap(), 2);
    assert_eq!(rx.recv().await.unwrap(), 3);

    let p = assert_some!(srx.recv().await);
    assert!(matches!(p, Message(s) if s.starts_with("0")));

    assert_err!(rx.try_recv());
}

#[tokio::test]
pub async fn connect_middleware_error() {
    let (_svc, io) = SocketIo::new_svc();
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
        { || {} }
            .with(handler(3, false))
            .with(handler(2, true))
            .with(handler(1, false)),
    );

    let (_, mut srx) = io.new_dummy_sock("/", ()).await;

    let p = assert_some!(srx.recv().await);
    assert_eq!(p, Message("4{\"message\":\"MyError\"}".to_string()));
    rx.recv().await.unwrap();
    rx.recv().await.unwrap();
    assert_err!(rx.try_recv());
}
