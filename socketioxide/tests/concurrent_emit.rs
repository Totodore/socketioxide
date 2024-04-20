//! Test for concurrent emit (issue https://github.com/Totodore/socketioxide/issues/232)
//! Binary messages are splitted into one string packet and adjacent binary packets.
//!
//! Under high load, if the atomicity of the emit is not guaranteed, binary packets may be out of order.
mod utils;

use bytes::Bytes;
use engineioxide::Packet::*;
use socketioxide::{extract::SocketRef, packet::create_binary_placeholder, SocketIo};

#[tokio::test]
pub async fn emit() {
    const BUFFER_SIZE: usize = 10000;
    let (_svc, io) = SocketIo::builder().max_buffer_size(BUFFER_SIZE).build_svc();
    io.ns("/", move |socket: SocketRef| async move {
        for _ in 0..100 {
            let s = socket.clone();
            tokio::task::spawn_blocking(move || {
                for _ in 0..100 {
                    s.bin(vec![
                        Bytes::from_static(&[1, 2, 3]),
                        Bytes::from_static(&[4, 5, 6]),
                    ])
                    .emit(
                        "test",
                        serde_json::Value::Array(vec![
                            "bin".into(),
                            create_binary_placeholder(0),
                            create_binary_placeholder(1),
                        ]),
                    )
                    .unwrap();
                }
            });
        }
    });

    let (_stx, mut srx) = io.new_dummy_sock("/", ()).await;
    assert_some!(srx.recv().await);

    let mut count = 0;
    let mut total = 0;
    const MSG: &str =
        r#"52-["test","bin",{"_placeholder":true,"num":0},{"_placeholder":true,"num":1}]"#;
    while let Some(msg) = srx.recv().await {
        match msg {
            Message(msg) if count == 0 && msg == MSG => {
                assert_eq!(msg, MSG);
                count = (count + 1) % 3;
                total += 1;
            }
            Binary(bin) if count == 1 => {
                assert_eq!(bin, Bytes::from_static(&[1, 2, 3]));
                count = (count + 1) % 3;
            }
            Binary(bin) if count == 2 => {
                assert_eq!(bin, Bytes::from_static(&[4, 5, 6]));
                count = (count + 1) % 3;
            }
            Ping | Pong | Message(_) | Close => (),
            msg => panic!("unexpected message: {:?}, count: {}", msg, count),
        };
        if total == BUFFER_SIZE {
            break;
        }
    }
}
