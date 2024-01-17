//! Adapters are responsible for managing the state of the server.
//! When a socket joins or leaves a room, the adapter is responsible for updating the state.
//! The default adapter is the [`LocalAdapter`], which stores the state in memory.
//! Other adapters can be made to share the state between multiple servers.

use std::{borrow::Cow, collections::HashSet, time::Duration};

use engineioxide::sid::Sid;

#[cfg(feature = "async-adapter")]
pub(crate) mod r#async;
mod local_adapter;
pub(crate) mod sync;

pub use local_adapter::LocalAdapter;
#[cfg(feature = "async-adapter")]
pub use r#async::Adapter;
#[cfg(not(feature = "async-adapter"))]
pub use sync::Adapter;

/// A room identifier
pub type Room = Cow<'static, str>;

/// Flags that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum BroadcastFlags {
    /// Broadcast only to the current server
    Local,
    /// Broadcast to all servers
    Broadcast,
    /// Add a custom timeout to the ack callback
    Timeout(Duration),
}

/// Options that can be used to modify the behavior of the broadcast methods.
#[derive(Clone, Debug)]
pub struct BroadcastOptions {
    /// The flags to apply to the broadcast.
    pub flags: HashSet<BroadcastFlags>,
    /// The rooms to broadcast to.
    pub rooms: HashSet<Room>,
    /// The rooms to exclude from the broadcast.
    pub except: HashSet<Room>,
    /// The socket id of the sender.
    pub sid: Option<Sid>,
}
impl BroadcastOptions {
    pub(crate) fn new(sid: Option<Sid>) -> Self {
        Self {
            flags: HashSet::new(),
            rooms: HashSet::new(),
            except: HashSet::new(),
            sid,
        }
    }
}
