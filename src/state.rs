use dashmap::DashMap;
use twilight_gateway::{shard::Events, Shard};
use twilight_model::{gateway::payload::incoming::Ready, guild::Guild, id::GuildId};

use std::{collections::HashMap, sync::Arc};

pub struct ShardStatus {
    pub shard: Shard,
    pub events: Events,
    pub first_time_used: bool,
    pub ready: Option<Ready>,
    pub guilds: HashMap<GuildId, Guild>,
}

// Each shard has the shard, event stream, first time connecting,
pub type Shards = DashMap<u64, ShardStatus>;

pub struct StateInner {
    pub shards: Shards,
    pub shard_count: u64,
}

pub type State = Arc<StateInner>;
