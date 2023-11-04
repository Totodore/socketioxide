use std::sync::Arc;

use serde::{Deserialize, Serialize};
use socketioxide::{adapter::Adapter, Socket};
use tracing::error;
use uuid::Uuid;

use crate::store::{get_sessions, Message, Session, MESSAGES};

#[derive(Debug, Deserialize)]
pub struct Auth {
    #[serde(rename = "sessionID")]
    session_id: Option<Uuid>,
    username: Option<String>,
}

/// Request/Response Types
#[derive(Debug, Serialize, Clone)]
struct UserConnectedRes {
    #[serde(rename = "userID")]
    user_id: Uuid,
    username: String,
    connected: bool,
    messages: Vec<Message>,
}

impl UserConnectedRes {
    fn new(session: &Session, messages: Vec<Message>) -> Self {
        Self {
            user_id: session.user_id,
            username: session.username.clone(),
            connected: session.connected,
            messages,
        }
    }
}
#[derive(Debug, Deserialize, Serialize, Clone)]
struct PrivateMessageReq {
    to: Uuid,
    content: String,
}

pub async fn on_connection<A: Adapter>(s: Arc<Socket<A>>, auth: Auth) {
    if let Err(e) = session_connect(&s, auth) {
        error!("Failed to connect: {:?}", e);
        s.disconnect().ok();
        return;
    }

    s.on(
        "private message",
        |s, PrivateMessageReq { to, content }, _, _| async move {
            let user_id = s.extensions.get::<Session>().unwrap().user_id;
            let message = Message {
                from: user_id,
                to,
                content,
            };
            MESSAGES.write().unwrap().push(message.clone());
            s.within(to.to_string())
                .emit("private message", message)
                .ok();
        },
    );

    s.on_disconnect(|s, _| async move {
        let mut session = s.extensions.get::<Session>().unwrap().clone();
        session.connected = false;

        get_sessions()
            .write()
            .unwrap()
            .get_mut(&session.session_id)
            .unwrap()
            .connected = false;

        s.broadcast().emit("user disconnected", session).ok();
    });
}

#[derive(Debug)]
enum ConnectError {
    InvalidUsername,
    EncodeError(serde_json::Error),
}

/// Handles the connection of a new user
fn session_connect<A: Adapter>(s: &Socket<A>, auth: Auth) -> Result<(), ConnectError> {
    let mut sessions = get_sessions().write().unwrap();
    if let Some(session) = auth.session_id.and_then(|id| sessions.get_mut(&id)) {
        session.connected = true;
        s.extensions.insert(session.clone());
    } else {
        let username = auth.username.ok_or(ConnectError::InvalidUsername)?;
        let session = Session::new(username);
        s.extensions.insert(session.clone());

        sessions.insert(session.session_id, session);
    };
    drop(sessions);

    let session = s.extensions.get::<Session>().unwrap();

    s.join(session.user_id.to_string()).ok();
    s.emit("session", session.clone())
        .map_err(ConnectError::EncodeError)?;

    let users = get_sessions()
        .read()
        .unwrap()
        .iter()
        .filter(|(id, _)| id != &&session.session_id)
        .map(|(_, session)| {
            let messages = MESSAGES
                .read()
                .unwrap()
                .iter()
                .filter(|message| message.to == session.user_id || message.from == session.user_id)
                .cloned()
                .collect();

            UserConnectedRes::new(session, messages)
        })
        .collect::<Vec<_>>();

    s.emit("users", [users])
        .map_err(ConnectError::EncodeError)?;

    let res = UserConnectedRes::new(&session, vec![]);

    s.broadcast()
        .emit("user connected", res)
        .map_err(ConnectError::EncodeError)?;
    Ok(())
}
