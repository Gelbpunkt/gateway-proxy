use serde::Deserialize;
use simd_json::OwnedValue;
use twilight_model::id::{ChannelId, GuildId};

#[derive(Deserialize)]
pub struct Identify {
    pub d: IdentifyInfo,
}

#[derive(Deserialize)]
pub struct VoiceStateUpdate {
    pub d: VoiceState,
}

#[derive(Deserialize)]
pub struct IdentifyInfo {
    #[serde(default)]
    pub compress: Option<bool>,
    pub shard: [u64; 2],
}

#[derive(Deserialize)]
pub struct VoiceState {
    pub guild_id: GuildId,
    #[serde(default)]
    pub channel_id: Option<ChannelId>,
}

#[derive(Deserialize)]
pub struct Ready {
    pub d: JsonObject,
}

pub type JsonObject = halfbrown::HashMap<String, OwnedValue>;
