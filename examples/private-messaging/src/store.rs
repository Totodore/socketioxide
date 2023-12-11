use std::{collections::HashMap, sync::RwLock};

use serde::Serialize;
use uuid::Uuid;

/// Store Types
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub connected: bool,
}
impl Session {
    pub fn new(username: String) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            username,
            connected: true,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub from: Uuid,
    pub to: Uuid,
    pub content: String,
}

#[derive(Default)]
pub struct Sessions(pub RwLock<HashMap<Uuid, Session>>);
#[derive(Default)]
pub struct Messages(pub RwLock<Vec<Message>>);
