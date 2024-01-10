use serde::Deserialize;
use serde_json::Value as OwnedValue;


#[derive(Deserialize)]
pub struct Identify {
    pub d: IdentifyInfo,
}

#[derive(Deserialize)]
pub struct Resume {
    pub d: ResumeInfo,
}

#[derive(Deserialize)]
pub struct IdentifyInfo {
    #[serde(default)]
    pub compress: Option<bool>,
    pub shard: [u32; 2],
    pub token: String,
}

#[derive(Deserialize)]
pub struct ResumeInfo {
    pub session_id: String,
    pub seq: usize,
    pub token: String,
}

#[derive(Deserialize)]
pub struct Ready {
    pub d: JsonObject,
}

pub type JsonObject = halfbrown::HashMap<String, OwnedValue>;
