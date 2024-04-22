mod utils;

use bytes::Bytes;
use engineioxide::Packet::*;
use socketioxide::{
    extract::{SocketRef, State},
    handler::ConnectHandler,
    packet::Packet,
    SendError, SocketError, SocketIo,
};
use tokio::sync::mpsc;

fn create_msg(ns: &str, event: &str, data: impl Into<serde_json::Value>) -> engineioxide::Packet {
    let packet: String = Packet::event(ns, event, data.into()).into();
    Message(packet.into())
}
async fn timeout_rcv<T: std::fmt::Debug>(srx: &mut tokio::sync::mpsc::Receiver<T>) -> T {
    tokio::time::timeout(std::time::Duration::from_millis(500), srx.recv())
        .await
        .unwrap()
        .unwrap()
}

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
                Err(SendError::Socket(SocketError::Closed(())))
            ));

            assert!(matches!(
                s.emit_with_ack::<(), ()>("test", ()),
                Err(SendError::Socket(SocketError::Closed(())))
            ));

            assert!(matches!(
                s.bin(vec![Bytes::from_static(&[0, 1, 2, 3])])
                    .emit("test", ()),
                Err(SendError::Socket(SocketError::Closed(())))
            ));
            assert!(matches!(
                s.bin(vec![Bytes::from_static(&[0, 1, 2, 3])])
                    .emit_with_ack::<(), ()>("test", ()),
                Err(SendError::Socket(SocketError::Closed(())))
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
    assert_eq!(p, Message("4{\"message\":\"MyError\"}".into()));
    rx.recv().await.unwrap();
    rx.recv().await.unwrap();
    assert_err!(rx.try_recv());
}

#[tokio::test]
async fn remove_ns() {
    use tokio::sync::mpsc::Sender;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(2);

    let (_svc, io) = SocketIo::builder().with_state(tx).build_svc();

    let io_clone = io.clone();
    let io_clone2 = io.clone();
    io.ns("/test1", move |s: SocketRef| {
        s.on("delete_ns", move |State::<Sender<()>>(tx)| {
            io_clone2.delete_ns("/test1");
            tx.try_send(()).unwrap();
        });
    });
    io.ns("/test2", move |s: SocketRef| {
        s.on_disconnect(move |State::<Sender<()>>(tx)| {
            io_clone.delete_ns("/test2");
            tx.try_send(()).unwrap();
        })
    });
    let (stx, mut srx) = io.new_dummy_sock("/test1", ()).await;
    timeout_rcv(&mut srx).await;
    assert_ok!(stx.try_send(create_msg("/test1", "delete_ns", ())));
    timeout_rcv(&mut rx).await;
    assert_ok!(stx.try_send(create_msg("/test1", "delete_ns", ())));
    // No response since ns is already deleted

    let (stx, mut srx) = io.new_dummy_sock("/test2", ()).await;
    timeout_rcv(&mut srx).await;
    assert_ok!(stx.try_send(Close));
    timeout_rcv(&mut rx).await;
}
