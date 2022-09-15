use rand::{distributions::Alphanumeric, thread_rng, Rng};
use tokio::sync::{broadcast, Notify};
use twilight_gateway::MessageSender;

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{cache, dispatch::BroadcastMessage, model::JsonObject};

/// Manager for the READY state of a shard.
pub struct Ready {
    inner: RwLock<Option<JsonObject>>,
    changed: Notify,
}

impl Ready {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
            changed: Notify::new(),
        }
    }

    pub async fn wait_changed(&self) {
        self.changed.notified().await;
    }

    pub fn is_ready(&self) -> bool {
        self.inner.read().unwrap().is_some()
    }

    pub fn set_ready(&self, payload: JsonObject) {
        *self.inner.write().unwrap() = Some(payload);
        self.changed.notify_waiters();
    }

    pub fn set_not_ready(&self) {
        *self.inner.write().unwrap() = None;
        self.changed.notify_waiters();
    }

    pub async fn wait_until_ready(&self) -> JsonObject {
        while !self.is_ready() {
            self.wait_changed().await;
        }

        self.inner.read().unwrap().clone().unwrap()
    }
}

/// State of a single shard.
pub struct Shard {
    /// ID of this shard.
    pub id: u64,
    /// Sender for this shard.
    pub sender: MessageSender,
    /// Handle for broadcasting events for this shard.
    pub events: broadcast::Sender<BroadcastMessage>,
    /// READY state manager for this shard.
    pub ready: Ready,
    /// Cache for guilds on this shard.
    pub guilds: cache::Guilds,
}

/// A session initiated by a client.
#[derive(Clone)]
pub struct Session {
    /// Shard ID that this session is for.
    pub shard_id: u64,
    /// Compression as requested in IDENTIFY.
    pub compress: Option<bool>,
}

/// Global state for all shards managed by the proxy.
pub struct Inner {
    /// State of all shards managed by the proxy.
    pub shards: Vec<Arc<Shard>>,
    /// Total shard count.
    pub shard_count: u64,
    /// All sessions active in the proxy.
    pub sessions: RwLock<HashMap<String, Session>>,
}

impl Inner {
    /// Get a session by its ID.
    pub fn get_session(&self, session_id: &str) -> Option<Session> {
        self.sessions.read().unwrap().get(session_id).cloned()
    }

    /// Create a new session.
    pub fn create_session(&self, session: Session) -> String {
        // Session IDs are 32 bytes of ASCII
        let mut rng = thread_rng();
        let session_id: String = std::iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .map(char::from)
            .take(32)
            .collect();

        self.sessions
            .write()
            .unwrap()
            .insert(session_id.clone(), session);

        session_id
    }
}

/// A reference to the [`StateInner`] of the proxy.
pub type State = Arc<Inner>;
