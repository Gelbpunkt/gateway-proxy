use tokio::sync::{broadcast, Notify};
use twilight_gateway::Shard;

use std::{lazy::SyncOnceCell, sync::Arc};

use crate::{cache::GuildCache, dispatch::BroadcastMessage, model::JsonObject};

pub struct ShardStatus {
    pub shard: Shard,
    pub events: broadcast::Sender<BroadcastMessage>,
    pub ready: SyncOnceCell<JsonObject>,
    pub ready_set: Notify,
    pub guilds: GuildCache,
}

pub type Shards = Vec<Arc<ShardStatus>>;

pub struct StateInner {
    pub shards: Shards,
    pub shard_count: u64,
}

pub type State = Arc<StateInner>;
