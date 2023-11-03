use std::sync::OnceLock;

use axum::Server;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use socketioxide::SocketIo;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

const TODOS: OnceLock<Vec<Todo>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Todo {
    completed: bool,
    editing: bool,
    title: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().with_line_number(true).finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::new_layer();
    TODOS.set(vec![]).unwrap();

    io.ns("/", |s, _: Value| async move {
        info!("New connection: {}", s.id);

        let todos = TODOS.get().cloned().unwrap_or_default();
        s.emit("todos", todos).unwrap();

        s.on("update-store", |s, new_todos: Vec<Todo>, _, _| async move {
            info!("Received update-store event: {:?}", new_todos);
            TODOS.set(new_todos.clone()).unwrap();
            s.broadcast().emit("update-store", new_todos).unwrap();
        });
    });

    let app = axum::Router::new()
        .nest_service("/", ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let server = Server::bind(&"0.0.0.0:8080".parse().unwrap()).serve(app.into_make_service());

    if let Err(e) = server.await {
        error!("server error: {}", e);
    }

    Ok(())
}
