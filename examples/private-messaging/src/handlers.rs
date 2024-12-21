use std::sync::{atomic::Ordering, Arc};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use socketioxide::extract::{Data, Extension, SocketRef, State};
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
struct UserConnectedRes<'a> {
    #[serde(rename = "userID")]
    user_id: &'a Uuid,
    username: &'a String,
    connected: bool,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize, Clone)]
struct UserDisconnectedRes<'a> {
    #[serde(rename = "userID")]
    user_id: &'a Uuid,
    username: &'a str,
}

impl<'a> UserConnectedRes<'a> {
    fn new(session: &'a Session, messages: Vec<Message>) -> Self {
        Self {
            user_id: &session.user_id,
            username: &session.username,
            connected: session.connected.load(Ordering::SeqCst),
            messages,
        }
    }
}
#[derive(Debug, Deserialize, Serialize, Clone)]
struct PrivateMessageReq {
    to: Uuid,
    content: String,
}

pub async fn on_connection(
    s: SocketRef,
    Extension::<Arc<Session>>(session): Extension<Arc<Session>>,
    State(sessions): State<Sessions>,
    State(msgs): State<Messages>,
) {
    s.emit("session", &*session).unwrap();

    let other_sessions = sessions.get_all_other_sessions(session.user_id);
    let users = other_sessions
        .iter()
        .map(|session| {
            let messages = msgs.get_all_for_user(session.user_id);
            UserConnectedRes::new(session, messages)
        })
        .collect::<Vec<_>>();

    s.emit("users", &users).unwrap();

    let res = UserConnectedRes::new(&session, vec![]);
    s.broadcast().emit("user connected", &res).await.unwrap();

    s.on(
        "private message",
        |s: SocketRef,
         Data(PrivateMessageReq { to, content }),
         State::<Messages>(msgs),
         Extension::<Arc<Session>>(session)| async move {
            let message = Message {
                from: session.user_id,
                to,
                content,
            };
            s.within(to.to_string())
                .emit("private message", &message)
                .await
                .ok();
            msgs.add(message);
        },
    );

    s.on_disconnect(
        |s: SocketRef, Extension::<Arc<Session>>(session)| async move {
            session.set_connected(false);
            let res = UserDisconnectedRes {
                user_id: &session.user_id,
                username: &session.username,
            };
            s.broadcast().emit("user disconnected", &res).await.ok();
        },
    );
}

/// Handles the connection of a new user.
/// Be careful to not emit anything to the user before the authentication is done.
pub async fn authenticate_middleware(
    s: SocketRef,
    Data(auth): Data<Auth>,
    State(sessions): State<Sessions>,
) -> Result<(), anyhow::Error> {
    let session = if let Some(session) = auth.session_id.and_then(|id| sessions.get(id)) {
        session.set_connected(true);
        s.extensions.insert(session.clone());
        session
    } else {
        let username = auth.username.ok_or(anyhow!("invalid username"))?;
        let session = Arc::new(Session::new(username));
        s.extensions.insert(session.clone());
        sessions.add(session.clone());
        session
    };

    s.join(session.user_id.to_string());

    Ok(())
}
