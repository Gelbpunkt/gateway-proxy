use serde::Deserialize;
use simd_json::OwnedValue;

#[derive(Deserialize)]
pub struct Identify {
    pub d: IdentifyInfo,
}

#[derive(Deserialize)]
pub struct IdentifyInfo {
    #[serde(default)]
    pub compress: Option<bool>,
    pub shard: [u64; 2],
    pub token: String,
}

#[derive(Deserialize)]
pub struct Ready {
    pub d: JsonObject,
}

pub type JsonObject = halfbrown::HashMap<String, OwnedValue>;
