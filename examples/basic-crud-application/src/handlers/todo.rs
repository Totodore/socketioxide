use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

use serde::{Deserialize, Serialize};
use socketioxide::extract::{AckSender, Data, SocketRef};
use tracing::info;
use uuid::Uuid;

use crate::handlers::events::Response;

use super::events::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    id: Uuid,
    #[serde(flatten)]
    inner: PartialTodo,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialTodo {
    completed: bool,
    title: String,
}

static TODOS: OnceLock<RwLock<HashMap<Uuid, Todo>>> = OnceLock::new();
fn get_store() -> &'static RwLock<HashMap<Uuid, Todo>> {
    TODOS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn create(s: SocketRef, Data(data): Data<PartialTodo>, ack: AckSender) {
    let id = Uuid::new_v4();
    let todo = Todo { id, inner: data };

    get_store().write().unwrap().insert(id, todo.clone());

    let res: Response<_> = id.into();
    ack.send(res).ok();

    s.broadcast().emit("todo:created", todo).ok();
}

pub async fn read(Data(id): Data<Uuid>, ack: AckSender) {
    let todos = get_store().read().unwrap();

    let todo = todos.get(&id).ok_or(Error::NotFound);
    ack.send(todo).ok();
}

pub async fn update(s: SocketRef, Data(data): Data<Todo>, ack: AckSender) {
    let mut todos = get_store().write().unwrap();
    let res = todos.get_mut(&data.id).ok_or(Error::NotFound).map(|todo| {
        todo.inner = data.inner.clone();
        s.broadcast().emit("todo:updated", data).ok();
    });

    ack.send(res).ok();
}

pub async fn delete(s: SocketRef, Data(id): Data<Uuid>, ack: AckSender) {
    let mut todos = get_store().write().unwrap();
    let res = todos.remove(&id).ok_or(Error::NotFound).map(|_| {
        s.broadcast().emit("todo:deleted", id).ok();
    });

    ack.send(res).ok();
}

pub async fn list(ack: AckSender) {
    let todos = get_store().read().unwrap();
    let res: Response<_> = todos.values().cloned().collect::<Vec<_>>().into();
    info!("Sending todos: {:?}", res);
    ack.send(res).ok();
}
