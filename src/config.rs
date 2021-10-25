use serde::Deserialize;
use twilight_gateway::Intents;
use twilight_model::gateway::presence::{Activity, Status};

use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    fs::read_to_string,
};

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    pub token: String,
    pub intents: Intents,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub shards: Option<u64>,
    #[serde(default)]
    pub activity: Option<Activity>,
    #[serde(default = "default_status")]
    pub status: Status,
}

fn default_log_level() -> String {
    String::from("info")
}

fn default_port() -> u16 {
    7878
}

fn default_status() -> Status {
    Status::Online
}

pub enum ConfigError {
    InvalidConfig(simd_json::Error),
    NotFound(String),
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidConfig(s) => s.fmt(f),
            Self::NotFound(s) => f.write_fmt(format_args!("File {} not found or access denied", s)),
        }
    }
}

pub fn load(path: &str) -> Result<Config, ConfigError> {
    let mut content = read_to_string(path).map_err(|_| ConfigError::NotFound(path.to_string()))?;
    let config = simd_json::from_str(&mut content).map_err(ConfigError::InvalidConfig)?;

    Ok(config)
}
