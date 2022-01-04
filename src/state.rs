use tokio::sync::{broadcast, watch};
use twilight_gateway::Shard as TwilightShard;

use std::sync::Arc;

use crate::{cache, dispatch::BroadcastMessage, model::JsonObject};

/// State of a single shard.
pub struct Shard {
    /// The [`TwilightShard`] this state is for.
    pub shard: TwilightShard,
    /// Handle for broadcasting events for this shard.
    pub events: broadcast::Sender<BroadcastMessage>,
    /// Receiver for READY payloads of this shard.
    pub ready: watch::Receiver<Option<JsonObject>>,
    /// Cache for guilds on this shard.
    pub guilds: cache::Guilds,
    /// Voice session cache for this shard.
    pub voice: cache::Voice,
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
