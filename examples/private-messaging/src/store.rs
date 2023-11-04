use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

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

static SESSIONS: OnceLock<RwLock<HashMap<Uuid, Session>>> = OnceLock::new();
pub fn get_sessions() -> &'static RwLock<HashMap<Uuid, Session>> {
    SESSIONS.get_or_init(|| RwLock::new(HashMap::new()))
}
pub static MESSAGES: RwLock<Vec<Message>> = RwLock::new(vec![]);
