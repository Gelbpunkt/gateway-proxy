use serde::Deserialize;

#[derive(Deserialize)]
pub struct Identify {
    pub d: IdentifyInfo,
}

#[derive(Deserialize)]
pub struct IdentifyInfo {
    pub shard: [u64; 2],
}
