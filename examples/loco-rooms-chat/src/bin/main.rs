use loco_chat_rooms::app::App;
use loco_rs::cli;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli::main::<App>().await
}
