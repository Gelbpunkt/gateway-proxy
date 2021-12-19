use serde::Deserialize;
use simd_json::OwnedValue;
use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker},
    Id,
};

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
    pub token: String,
}

#[derive(Deserialize)]
pub struct VoiceState {
    pub guild_id: Id<GuildMarker>,
    #[serde(default)]
    pub channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Deserialize)]
pub struct Ready {
    pub d: JsonObject,
}

pub type JsonObject = halfbrown::HashMap<String, OwnedValue>;
