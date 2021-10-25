use dashmap::DashMap;
use twilight_gateway::{shard::Events, Shard};
use twilight_model::gateway::payload::incoming::Ready;

use std::sync::Arc;

use crate::cache::GuildCache;

pub struct ShardStatus {
    pub shard: Shard,
    pub events: Events,
    pub first_time_used: bool,
    pub ready: Option<Ready>,
    pub guilds: GuildCache,
}

// Each shard has the shard, event stream, first time connecting,
pub type Shards = DashMap<u64, ShardStatus>;

pub struct StateInner {
    pub shards: Shards,
    pub shard_count: u64,
}

pub type State = Arc<StateInner>;
