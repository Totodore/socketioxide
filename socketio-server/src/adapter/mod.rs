mod adapter;
pub use adapter::{BroadcastFlags, BroadcastOptions, RoomParam, Room};
#[cfg(feature = "remote_adapter")]
pub use adapter::AsyncAdapter as Adapter;
#[cfg(not(feature = "remote_adapter"))]
pub use adapter::SyncAdapter as Adapter;

mod local_adapter;
pub use local_adapter::LocalAdapter;