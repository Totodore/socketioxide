use loco_rs::cli;
use loco_chat_rooms::app::App;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli::main::<App>().await
}
