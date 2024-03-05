use std::{pin::Pin, time::Duration};

use rust_socketio::{
    asynchronous::{Client, ClientBuilder},
    Payload,
};

const PING_INTERVAL: Duration = Duration::from_millis(1000);
const POLLING_PERCENTAGE: f32 = 0.05;
const MAX_CLIENT: usize = 200;

fn cb(_: Payload, socket: Client) -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        tokio::spawn(async move {
            let mut inter = tokio::time::interval(PING_INTERVAL);
            loop {
                inter.tick().await;
                let _ = socket.emit("ping", serde_json::Value::Null).await;
                let _ = socket
                    .emit("ping", (0..u8::MAX).into_iter().collect::<Vec<u8>>())
                    .await;
            }
        });
    })
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::spawn(async move {
        for _ in 0..MAX_CLIENT {
            let random: f32 = rand::random();
            let transport_type = if POLLING_PERCENTAGE > random {
                rust_socketio::TransportType::Polling
            } else {
                rust_socketio::TransportType::WebsocketUpgrade
            };
            // get a socket that is connected to the admin namespace
            ClientBuilder::new("http://localhost:3000/")
                .transport_type(transport_type)
                .namespace("/")
                .on("open", cb)
                .on("error", |err, _| {
                    Box::pin(async move { eprintln!("Error: {:#?}", err) })
                })
                .connect()
                .await
                .expect("Connection failed");
        }
    });
    tokio::time::sleep(Duration::from_secs(60)).await;

    Ok(())
}
