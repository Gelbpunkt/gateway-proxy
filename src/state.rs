use tokio::sync::{broadcast, watch};
use twilight_gateway::Shard;

use std::sync::Arc;

use crate::{
    cache::{GuildCache, VoiceCache},
    dispatch::BroadcastMessage,
    model::JsonObject,
};

/// State of a single shard.
pub struct ShardState {
    /// The [`Shard`] this state is for.
    pub shard: Shard,
    /// Handle for broadcasting events for this shard.
    pub events: broadcast::Sender<BroadcastMessage>,
    /// Receiver for READY payloads of this shard.
    pub ready: watch::Receiver<Option<JsonObject>>,
    /// Cache for guilds on this shard.
    pub guilds: GuildCache,
    /// Voice session cache for this shard.
    pub voice: VoiceCache,
}

/// Global state for all shards managed by the proxy.
pub struct InnerState {
    /// State of all shards managed by the proxy.
    pub shards: Vec<Arc<ShardState>>,
    /// Total shard count.
    pub shard_count: u64,
}

/// A reference to the [`StateInner`] of the proxy.
pub type State = Arc<InnerState>;
