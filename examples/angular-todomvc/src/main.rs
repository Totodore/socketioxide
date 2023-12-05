use std::sync::Mutex;

use axum::Server;

use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{Data, SocketRef, State},
    SocketIo,
};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Todo {
    completed: bool,
    editing: bool,
    title: String,
}

type Todos = Mutex<Vec<Todo>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    let todos: Todos = Mutex::new(vec![]);
    let (layer, io) = SocketIo::builder().with_state(todos).build_layer();

    io.ns("/", |s: SocketRef, todos: State<Todos>| {
        info!("New connection: {}", s.id);

        let todos = todos.lock().unwrap().clone();

        // Because variadic args are not supported, array arguments are flattened.
        // Therefore to send a json array (required for the todomvc app) we need to wrap it in another array.
        s.emit("todos", [todos]).unwrap();

        s.on(
            "update-store",
            |s: SocketRef, Data::<Vec<Todo>>(new_todos), todos: State<Todos>| {
                info!("Received update-store event: {:?}", new_todos);

                let mut todos = todos.lock().unwrap();
                todos.clear();
                todos.extend_from_slice(&new_todos);

                s.broadcast().emit("update-store", [new_todos]).unwrap();
            },
        );
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
