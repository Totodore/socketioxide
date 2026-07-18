use engineioxide_client::{Client, EioEvent};
use futures_util::{SinkExt, StreamExt};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let client = Client::connect_with_hyper_ws("http://localhost:3000/engine.io").await?;
    let (mut tx, mut rx) = client.split();

    while let Some(Ok(event)) = rx.next().await {
        match event {
            EioEvent::Connect(sid) => {
                println!("socket connect {sid}");
            }
            EioEvent::Disconnect => {
                println!("socket disconnect");
            }
            EioEvent::Message(msg) => {
                println!("Ping pong message {:?}", msg);
                tx.send(EioEvent::Message(msg)).await?;
            }
            EioEvent::Binary(data) => {
                println!("Ping pong binary message {:?}", data);
                tx.send(EioEvent::Binary(data)).await?;
            }
            EioEvent::Upgrade(transport_type) => {
                println!("transport upgraded to {transport_type:?}")
            }
        }
    }
    Ok(())
}
