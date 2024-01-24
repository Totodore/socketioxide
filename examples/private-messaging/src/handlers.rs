use serde::{Deserialize, Serialize};
use socketioxide::extract::{Data, SocketRef, State, TryData};
use tracing::error;
use uuid::Uuid;

use crate::store::{Message, Messages, Session, Sessions};

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

pub fn on_connection(
    s: SocketRef,
    TryData(auth): TryData<Auth>,
    sessions: State<Sessions>,
    msgs: State<Messages>,
) {
    if let Err(e) = session_connect(&s, auth, sessions.0, msgs.0) {
        error!("Failed to connect: {:?}", e);
        s.disconnect().ok();
        return;
    }

    s.on(
        "private message",
        |s: SocketRef, Data(PrivateMessageReq { to, content }), State(Messages(msg))| {
            let user_id = s.extensions.get::<Session>().unwrap().user_id;
            let message = Message {
                from: user_id,
                to,
                content,
            };
            msg.write().unwrap().push(message.clone());
            s.within(to.to_string())
                .emit("private message", message)
                .ok();
        },
    );

    s.on_disconnect(|s: SocketRef, State(Sessions(sessions))| {
        let mut session = s.extensions.get::<Session>().unwrap().clone();
        session.connected = false;

        sessions
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
    SocketError(socketioxide::SendError),
    BroadcastError(socketioxide::BroadcastError),
}

/// Handles the connection of a new user
fn session_connect(
    s: &SocketRef,
    auth: Result<Auth, serde_json::Error>,
    Sessions(session_state): &Sessions,
    Messages(msg_state): &Messages,
) -> Result<(), ConnectError> {
    let auth = auth.map_err(ConnectError::EncodeError)?;
    let mut sessions = session_state.write().unwrap();
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
        .map_err(ConnectError::SocketError)?;

    let users = session_state
        .read()
        .unwrap()
        .iter()
        .filter(|(id, _)| id != &&session.session_id)
        .map(|(_, session)| {
            let messages = msg_state
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
        .map_err(ConnectError::SocketError)?;

    let res = UserConnectedRes::new(&session, vec![]);

    s.broadcast()
        .emit("user connected", res)
        .map_err(ConnectError::BroadcastError)?;
    Ok(())
}
