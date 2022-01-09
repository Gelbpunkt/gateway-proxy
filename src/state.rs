use parking_lot::RwLock;
use tokio::sync::{broadcast, Notify};
use twilight_gateway::Shard as TwilightShard;

use std::sync::Arc;

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
        self.inner.read().is_some()
    }

    pub fn set_ready(&self, payload: JsonObject) {
        *self.inner.write() = Some(payload);
        self.changed.notify_waiters();
    }

    pub fn set_not_ready(&self) {
        *self.inner.write() = None;
        self.changed.notify_waiters();
    }

    pub async fn wait_until_ready(&self) -> JsonObject {
        while !self.is_ready() {
            self.wait_changed().await;
        }

        self.inner.read().clone().unwrap()
    }
}

/// State of a single shard.
pub struct Shard {
    /// The [`TwilightShard`] this state is for.
    pub shard: TwilightShard,
    /// Handle for broadcasting events for this shard.
    pub events: broadcast::Sender<BroadcastMessage>,
    /// READY state manager for this shard.
    pub ready: Ready,
    /// Cache for guilds on this shard.
    pub guilds: cache::Guilds,
}

/// Global state for all shards managed by the proxy.
pub struct Inner {
    /// State of all shards managed by the proxy.
    pub shards: Vec<Arc<Shard>>,
    /// Total shard count.
    pub shard_count: u64,
}

/// A reference to the [`StateInner`] of the proxy.
pub type State = Arc<Inner>;
