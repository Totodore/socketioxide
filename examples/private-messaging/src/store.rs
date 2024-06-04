use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc, RwLock},
};

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

/// Store Types
#[derive(Debug, Serialize)]
pub struct Session {
    #[serde(rename = "sessionID")]
    pub session_id: Uuid,
    #[serde(rename = "userID")]
    pub user_id: Uuid,
    pub username: String,
    pub connected: AtomicBool,
}
impl Session {
    pub fn new(username: String) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            username,
            connected: AtomicBool::new(true),
        }
    }
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
    }
}
impl Clone for Session {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            username: self.username.clone(),
            connected: AtomicBool::new(self.connected.load(Ordering::SeqCst)),
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
pub struct Sessions(RwLock<HashMap<Uuid, Arc<Session>>>);

impl Sessions {
    pub fn get_all_other_sessions(&self, user_id: Uuid) -> Vec<Arc<Session>> {
        self.0
            .read()
            .unwrap()
            .values()
            .filter(|s| s.user_id != user_id)
            .cloned()
            .collect()
    }

    pub fn get(&self, user_id: Uuid) -> Option<Arc<Session>> {
        self.0.read().unwrap().get(&user_id).cloned()
    }

    pub fn add(&self, session: Arc<Session>) {
        self.0.write().unwrap().insert(session.session_id, session);
    }
}
#[derive(Default)]
pub struct Messages(RwLock<Vec<Message>>);

impl Messages {
    pub fn add(&self, message: Message) {
        self.0.write().unwrap().push(message);
    }

    pub fn get_all_for_user(&self, user_id: Uuid) -> Vec<Message> {
        self.0
            .read()
            .unwrap()
            .iter()
            .filter(|m| m.from == user_id || m.to == user_id)
            .cloned()
            .collect()
    }
}
