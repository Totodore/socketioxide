use std::time::Duration;

use serde::{Deserialize, Serialize};
use socketioxide::{
    adapter::Adapter,
    extract::{Data, Extension, SocketRef, State},
    SocketIo,
};
use socketioxide_mongodb::{
    drivers::mongodb::mongodb_client::{
        self as mongodb,
        bson::{doc, oid::ObjectId, Document},
        options::ReturnDocument,
        Collection, Database,
    },
    MessageExpirationStrategy, MongoDbAdapter, MongoDbAdapterConfig, MongoDbAdapterCtr,
};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

#[derive(Deserialize, Serialize, Debug)]
struct ChatDocument {
    _id: ObjectId,
    remote_user_count: usize,
}
impl ChatDocument {
    const COLLECTION_NAME: &'static str = "chat";
    const ID: ObjectId = make_object_id(
        [0x66, 0x2A, 0x15, 0x49],
        [0xE9, 0x2C, 0x3C, 0x8D],
        [0x5E, 0x8B],
    );
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(transparent)]
struct Username(String);

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase", untagged)]
enum Res {
    Login {
        #[serde(rename = "numUsers")]
        num_users: usize,
    },
    UserEvent {
        #[serde(rename = "numUsers")]
        num_users: usize,
        username: Username,
    },
    Message {
        username: Username,
        message: String,
    },
    Username {
        username: Username,
    },
}
#[derive(Clone)]
struct RemoteUserCnt {
    collec: Collection<ChatDocument>,
}
impl RemoteUserCnt {
    fn new(conn: Database) -> Self {
        Self {
            collec: conn.collection(ChatDocument::COLLECTION_NAME),
        }
    }

    async fn add_user(&self) -> Result<usize, mongodb::error::Error> {
        let where_doc: Document = doc! {
            "_id": ChatDocument::ID,
        };
        let doc = self
            .collec
            .find_one_and_update(where_doc, doc! { "$inc": { "remote_user_count": 1 } })
            .return_document(ReturnDocument::After)
            .await?;
        Ok(doc.map_or(0, |doc| doc.remote_user_count))
    }
    async fn remove_user(&self) -> Result<usize, mongodb::error::Error> {
        let where_doc: Document = doc! {
            "_id": ChatDocument::ID,
        };
        let doc = self
            .collec
            .find_one_and_update(where_doc, doc! { "$dec": { "remote_user_count": 1 } })
            .return_document(ReturnDocument::After)
            .await?;
        Ok(doc.map_or(0, |doc| doc.remote_user_count))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::new();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting server");

    const URI: &str = "mongodb://127.0.0.1:27017/?replicaSet=rs0&directConnection=true";
    const DB: &str = "chat";
    let db = mongodb::Client::with_uri_str(URI).await?.database(DB);

    // This is the default expiration strategy for messages in the database.
    let strategy = MessageExpirationStrategy::TtlIndex(Duration::from_secs(60));
    let config = MongoDbAdapterConfig::new()
        .with_expiration_strategy(strategy)
        .with_collection("socket.io-adapter-ttl");
    let adapter = MongoDbAdapterCtr::new_with_mongodb_config(db.clone(), config).await?;
    let (layer, io) = SocketIo::builder()
        .with_state(RemoteUserCnt::new(db))
        .with_adapter::<MongoDbAdapter<_>>(adapter)
        .build_layer();
    io.ns("/", on_connect).await?;

    let app = axum::Router::new()
        .fallback_service(ServeDir::new("dist"))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::permissive()) // Enable CORS policy
                .layer(layer),
        );

    let port = std::env::var("PORT")
        .map(|s| s.parse().unwrap())
        .unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn on_connect<A: Adapter>(socket: SocketRef<A>) {
    socket.on("new message", on_msg);
    socket.on("add user", on_add_user);
    socket.on("typing", on_typing);
    socket.on("stop typing", on_stop_typing);
    socket.on_disconnect(on_disconnect);
}
async fn on_msg<A: Adapter>(
    s: SocketRef<A>,
    Data(msg): Data<String>,
    Extension(username): Extension<Username>,
) {
    let msg = &Res::Message {
        username,
        message: msg,
    };
    s.broadcast().emit("new message", msg).await.ok();
}
async fn on_add_user<A: Adapter>(
    s: SocketRef<A>,
    Data(username): Data<String>,
    user_cnt: State<RemoteUserCnt>,
) {
    if s.extensions.get::<Username>().is_some() {
        return;
    }
    let num_users = user_cnt.add_user().await.unwrap_or(0);
    s.extensions.insert(Username(username.clone()));
    s.emit("login", &Res::Login { num_users }).ok();

    let res = &Res::UserEvent {
        num_users,
        username: Username(username),
    };
    s.broadcast().emit("user joined", res).await.ok();
}
async fn on_typing<A: Adapter>(s: SocketRef<A>, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("typing", &Res::Username { username })
        .await
        .ok();
}
async fn on_stop_typing<A: Adapter>(s: SocketRef<A>, Extension(username): Extension<Username>) {
    s.broadcast()
        .emit("stop typing", &Res::Username { username })
        .await
        .ok();
}
async fn on_disconnect<A: Adapter>(
    s: SocketRef<A>,
    user_cnt: State<RemoteUserCnt>,
    Extension(username): Extension<Username>,
) {
    let num_users = user_cnt.remove_user().await.unwrap_or(0);
    let res = &Res::UserEvent {
        num_users,
        username,
    };
    s.broadcast().emit("user left", res).await.ok();
}

/// Creates a new ObjectId at comp time with the given timestamp, machine ID, and process ID.
/// This is a hacky way to have a unique object ID accross instances but it shouldn't be used in
/// real-world production environments.
const fn make_object_id(timestamp: [u8; 4], machine: [u8; 4], pid: [u8; 2]) -> ObjectId {
    ObjectId::from_bytes([
        timestamp[0],
        timestamp[1],
        timestamp[2],
        timestamp[3],
        machine[0],
        machine[1],
        machine[2],
        machine[3],
        pid[0],
        pid[1],
        0,
        0,
    ])
}
