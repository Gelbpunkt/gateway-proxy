use tokio::sync::{broadcast, watch};
use twilight_gateway::Shard;

use std::sync::Arc;

use crate::{
    cache::{GuildCache, VoiceCache},
    dispatch::BroadcastMessage,
    model::JsonObject,
};

pub struct ShardStatus {
    pub shard: Shard,
    pub events: broadcast::Sender<BroadcastMessage>,
    pub ready: watch::Receiver<Option<JsonObject>>,
    pub guilds: GuildCache,
    pub voice: VoiceCache,
}

pub type Shards = Vec<Arc<ShardStatus>>;

pub struct StateInner {
    pub shards: Shards,
    pub shard_count: u64,
}

pub type State = Arc<StateInner>;
