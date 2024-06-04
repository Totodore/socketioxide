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
struct UserConnectedRes {
    #[serde(rename = "userID")]
    user_id: Uuid,
    username: String,
    connected: bool,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize, Clone)]
struct UserDisconnectedRes {
    #[serde(rename = "userID")]
    user_id: Uuid,
    username: String,
}

impl UserConnectedRes {
    fn new(session: &Session, messages: Vec<Message>) -> Self {
        Self {
            user_id: session.user_id,
            username: session.username.clone(),
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

pub fn on_connection(
    s: SocketRef,
    Extension::<Arc<Session>>(session): Extension<Arc<Session>>,
    State(sessions): State<Sessions>,
    State(msgs): State<Messages>,
) {
    s.emit("session", (*session).clone()).unwrap();

    let users = sessions
        .get_all_other_sessions(session.user_id)
        .into_iter()
        .map(|session| {
            let messages = msgs.get_all_for_user(session.user_id);
            UserConnectedRes::new(&session, messages)
        })
        .collect::<Vec<_>>();

    s.emit("users", [users]).unwrap();

    let res = UserConnectedRes::new(&session, vec![]);
    s.broadcast().emit("user connected", res).unwrap();

    s.on(
        "private message",
        |s: SocketRef,
         Data(PrivateMessageReq { to, content }),
         State::<Messages>(msgs),
         Extension::<Arc<Session>>(session)| {
            let message = Message {
                from: session.user_id,
                to,
                content,
            };
            msgs.add(message.clone());
            s.within(to.to_string())
                .emit("private message", message)
                .ok();
        },
    );

    s.on_disconnect(|s: SocketRef, Extension::<Arc<Session>>(session)| {
        session.set_connected(false);
        s.broadcast()
            .emit(
                "user disconnected",
                UserDisconnectedRes {
                    user_id: session.user_id,
                    username: session.username.clone(),
                },
            )
            .ok();
    });
}

/// Handles the connection of a new user.
/// Be careful to not emit anything to the user before the authentication is done.
pub fn authenticate_middleware(
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

    s.join(session.user_id.to_string())?;

    Ok(())
}
