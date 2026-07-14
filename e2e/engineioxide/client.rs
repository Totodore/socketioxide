use engineioxide_client::{Client, EioEvent};
use futures_util::{SinkExt, StreamExt};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_line_number(true)
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    let client = Client::connect_with_hyper().await?;
    let (mut tx, mut rx) = client.split();

    tx.send(EioEvent::Message("hello".into())).await?;
    while let Some(Ok(event)) = rx.next().await {
        match event {
            EioEvent::Connect => {
                println!("socket connect");
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
