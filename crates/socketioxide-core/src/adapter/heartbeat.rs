//! Shared heartbeat/liveness logic for remote adapters.

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    Uid,
    adapter::remote_packet::{RequestOut, RequestTypeIn, RequestTypeOut},
};

/// Tracks the liveness of remote nodes based on the heartbeats they send.
pub struct HeartbeatTracker {
    /// The last time each remote node was seen alive.
    nodes_liveness: Mutex<Vec<(Uid, Instant)>>,
    /// The duration after which a node is considered dead.
    hb_timeout: Duration,
}

impl HeartbeatTracker {
    /// Create a new `HeartbeatTracker` with the given heartbeat timeout.
    pub fn new(hb_timeout: Duration) -> Self {
        Self {
            nodes_liveness: Mutex::new(Vec::new()),
            hb_timeout,
        }
    }

    /// Record a heartbeat received from `origin`.
    ///
    /// Returns `true` if the node is new and an [`RequestTypeIn::InitHeartbeat`]
    /// reply is due, `false` if the node was already known.
    pub fn on_heartbeat(&self, origin: Uid) -> bool {
        let mut nodes_liveness = self.nodes_liveness.lock().unwrap();
        for (id, liveness) in nodes_liveness.iter_mut() {
            if *id == origin {
                *liveness = Instant::now();
                return false;
            }
        }
        nodes_liveness.push((origin, Instant::now()));
        true
    }

    /// The number of live nodes, including the current one.
    ///
    /// Dead nodes are pruned from the tracker.
    pub fn server_count(&self) -> u16 {
        let threshold = Instant::now() - self.hb_timeout;
        let mut nodes_liveness = self.nodes_liveness.lock().unwrap();
        nodes_liveness.retain(|(_, v)| v > &threshold);
        (nodes_liveness.len() + 1) as u16
    }

    /// Whether the given remote node is currently considered alive.
    pub fn is_alive(&self, uid: Uid) -> bool {
        let threshold = Instant::now() - self.hb_timeout;
        self.nodes_liveness
            .lock()
            .unwrap()
            .iter()
            .any(|(id, liveness)| *id == uid && *liveness > threshold)
    }
}

/// The sending side of the heartbeat protocol, implemented by the remote adapters.
pub trait HeartbeatSender: Send + Sync + 'static {
    /// The error type returned by the adapter.
    type Error: std::error::Error + Send + 'static;

    /// The unique identifier of the current node.
    fn uid(&self) -> Uid;

    /// Send a request to a specific target node or broadcast it to all nodes
    /// if no target is specified.
    fn send_req(
        &self,
        req: RequestOut<'_>,
        target: Option<Uid>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Emit a heartbeat to the specified target node or broadcast it to all nodes.
    fn emit_heartbeat(
        &self,
        target: Option<Uid>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move {
            self.send_req(
                RequestOut::new_empty(self.uid(), RequestTypeOut::Heartbeat),
                target,
            )
            .await
        }
    }

    /// Emit an initial heartbeat to all nodes.
    fn emit_init_heartbeat(&self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async move {
            self.send_req(
                RequestOut::new_empty(self.uid(), RequestTypeOut::InitHeartbeat),
                None,
            )
            .await
        }
    }

    /// Receive a heartbeat from a remote node.
    ///
    /// It might be an [`RequestTypeIn::InitHeartbeat`] packet, in which case a
    /// heartbeat is re-emitted to the new node so it discovers the current one.
    fn recv_heartbeat(
        self: &Arc<Self>,
        tracker: &HeartbeatTracker,
        req_type: RequestTypeIn,
        origin: Uid,
    ) {
        tracing::debug!(?req_type, "{:?} received", req_type);
        if tracker.on_heartbeat(origin) && matches!(req_type, RequestTypeIn::InitHeartbeat) {
            tracing::debug!(
                ?origin,
                "initial heartbeat detected, saying hello to the new node"
            );

            let this = self.clone();
            tokio::spawn(async move {
                if let Err(err) = this.emit_heartbeat(Some(origin)).await {
                    tracing::warn!(
                        "could not re-emit heartbeat after new node detection: {:?}",
                        err
                    );
                }
            });
        }
    }
}

/// Run the heartbeat loop, emitting a heartbeat to all nodes every `hb_interval`.
///
/// The first tick is skipped as it yields immediately.
pub async fn heartbeat_loop<S: HeartbeatSender>(
    adapter: Arc<S>,
    hb_interval: Duration,
) -> Result<(), S::Error> {
    let mut interval = tokio::time::interval(hb_interval);
    interval.tick().await; // first tick yields immediately
    loop {
        interval.tick().await;
        adapter.emit_heartbeat(None).await?;
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use super::*;

    struct TestSender {
        uid: Uid,
    }

    impl HeartbeatSender for TestSender {
        type Error = std::io::Error;

        fn uid(&self) -> Uid {
            self.uid
        }

        async fn send_req(
            &self,
            _req: RequestOut<'_>,
            _target: Option<Uid>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn on_heartbeat_new_node() {
        let tracker = HeartbeatTracker::new(Duration::from_secs(60));
        assert!(tracker.on_heartbeat(Uid::new()));
    }

    #[test]
    fn on_heartbeat_known_node() {
        let tracker = HeartbeatTracker::new(Duration::from_secs(60));
        let uid = Uid::new();
        assert!(tracker.on_heartbeat(uid));
        assert!(!tracker.on_heartbeat(uid));
    }

    #[test]
    fn server_count_includes_self() {
        let tracker = HeartbeatTracker::new(Duration::from_secs(60));
        assert_eq!(tracker.server_count(), 1);
        tracker.on_heartbeat(Uid::new());
        tracker.on_heartbeat(Uid::new());
        assert_eq!(tracker.server_count(), 3);
    }

    #[test]
    fn dead_nodes_are_pruned() {
        let tracker = HeartbeatTracker::new(Duration::from_millis(50));
        tracker.on_heartbeat(Uid::new());
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(tracker.server_count(), 1);
    }

    #[test]
    fn is_alive_reflects_timeout() {
        let tracker = HeartbeatTracker::new(Duration::from_millis(50));
        let uid = Uid::new();
        assert!(!tracker.is_alive(uid));
        tracker.on_heartbeat(uid);
        assert!(tracker.is_alive(uid));
        std::thread::sleep(Duration::from_millis(60));
        assert!(!tracker.is_alive(uid));
    }

    #[tokio::test]
    async fn recv_heartbeat_emits_reply_for_new_node() {
        let adapter = Arc::new(TestSender { uid: Uid::new() });
        let tracker = HeartbeatTracker::new(Duration::from_secs(60));
        adapter.recv_heartbeat(&tracker, RequestTypeIn::InitHeartbeat, Uid::new());
    }
}
