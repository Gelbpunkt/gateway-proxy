use serde::Deserialize;
use twilight_cache_inmemory::ResourceType;
use twilight_gateway::{EventTypeFlags, Intents};
use twilight_model::gateway::presence::{Activity, Status};

use std::{
    convert::Into,
    fmt::{Display, Formatter, Result as FmtResult},
    fs::read_to_string,
    lazy::SyncLazy,
    process::exit,
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
    #[serde(default = "default_backpressure")]
    pub backpressure: usize,
    #[serde(default)]
    pub twilight_http_proxy: Option<String>,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Deserialize, Clone)]
pub struct CacheConfig {
    pub channels: bool,
    pub presences: bool,
    pub emojis: bool,
    pub current_member: bool,
    pub members: bool,
    pub roles: bool,
    pub stage_instances: bool,
    pub stickers: bool,
    pub users: bool,
    pub voice_states: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            channels: true,
            presences: false,
            current_member: true,
            emojis: false,
            members: false,
            roles: true,
            stage_instances: false,
            stickers: false,
            users: false,
            voice_states: false,
        }
    }
}

impl Into<EventTypeFlags> for CacheConfig {
    fn into(self) -> EventTypeFlags {
        let mut flags = EventTypeFlags::SHARD_PAYLOAD
            | EventTypeFlags::GUILD_CREATE
            | EventTypeFlags::GUILD_DELETE
            | EventTypeFlags::GUILD_UPDATE
            | EventTypeFlags::READY
            | EventTypeFlags::SHARD_RECONNECTING;

        if self.members || self.current_member {
            flags |= EventTypeFlags::MEMBER_ADD
                | EventTypeFlags::MEMBER_REMOVE
                | EventTypeFlags::MEMBER_UPDATE;
        }

        if self.roles {
            flags |= EventTypeFlags::ROLE_CREATE
                | EventTypeFlags::ROLE_DELETE
                | EventTypeFlags::ROLE_UPDATE;
        }

        if self.channels {
            flags |= EventTypeFlags::CHANNEL_CREATE
                | EventTypeFlags::CHANNEL_DELETE
                | EventTypeFlags::CHANNEL_UPDATE;
        }

        if self.presences {
            flags |= EventTypeFlags::PRESENCE_UPDATE;
        }

        if self.emojis {
            flags |= EventTypeFlags::GUILD_EMOJIS_UPDATE;
        }

        if self.stage_instances {
            flags |= EventTypeFlags::STAGE_INSTANCE_CREATE
                | EventTypeFlags::STAGE_INSTANCE_DELETE
                | EventTypeFlags::STAGE_INSTANCE_UPDATE;
        }

        if self.voice_states {
            flags |= EventTypeFlags::VOICE_STATE_UPDATE | EventTypeFlags::VOICE_SERVER_UPDATE;
        }

        if self.users {
            flags |= EventTypeFlags::USER_UPDATE;
        }

        flags
    }
}

impl Into<ResourceType> for CacheConfig {
    fn into(self) -> ResourceType {
        let mut resource_types = ResourceType::GUILD | ResourceType::USER_CURRENT;

        if self.channels {
            resource_types |= ResourceType::CHANNEL;
        }

        if self.emojis {
            resource_types |= ResourceType::EMOJI;
        }

        if self.current_member {
            resource_types |= ResourceType::MEMBER_CURRENT;
        }

        if self.members {
            resource_types |= ResourceType::MEMBER;
        }

        if self.presences {
            resource_types |= ResourceType::PRESENCE;
        }

        if self.roles {
            resource_types |= ResourceType::ROLE;
        }

        if self.stage_instances {
            resource_types |= ResourceType::STAGE_INSTANCE;
        }

        if self.stickers {
            resource_types |= ResourceType::STICKER;
        }

        if self.users {
            resource_types |= ResourceType::USER;
        }

        if self.voice_states {
            resource_types |= ResourceType::VOICE_STATE;
        }

        resource_types
    }
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

fn default_backpressure() -> usize {
    100
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

pub static CONFIG: SyncLazy<Config> = SyncLazy::new(|| {
    match load("config.json") {
        Ok(config) => config,
        Err(err) => {
            // Avoid panicking
            eprintln!("Config Error: {}", err);
            exit(1);
        }
    }
});
