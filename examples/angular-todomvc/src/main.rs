use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{Data, SocketRef, State},
    SocketIo,
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(Default, Clone)]
struct Todos(Arc<Mutex<Vec<Todo>>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Todo {
    completed: bool,
    editing: bool,
    title: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let (layer, io) = SocketIo::builder()
        .with_state(Todos::default())
        .build_layer();

    io.ns("/", |s: SocketRef, State(Todos(todos))| {
        info!("New connection: {}", s.id);

        let todos = todos.lock().unwrap().clone();

        s.emit("todos", &todos).unwrap();

        s.on(
            "update-store",
            |s: SocketRef, Data::<Vec<Todo>>(new_todos), State(Todos(todos))| {
                info!("Received update-store event: {:?}", new_todos);

                let mut todos = todos.lock().unwrap();
                todos.clear();
                todos.extend_from_slice(&new_todos);
                async move {
                    s.broadcast()
                        .emit("update-store", &new_todos)
                        .await
                        .unwrap();
                }
            },
        );
    });

    let app = axum::Router::new()
        .fallback_service(ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
