use serde::Deserialize;
#[cfg(not(feature = "simd-json"))]
use serde_json::Error as JsonError;
#[cfg(feature = "simd-json")]
use simd_json::Error as JsonError;
use twilight_cache_inmemory::ResourceType;
use twilight_gateway::{EventTypeFlags, Intents};
use twilight_model::gateway::presence::{Activity, Status};

use std::{
    env::var,
    fmt::{Display, Formatter, Result as FmtResult},
    fs::read_to_string,
    process::exit,
    sync::LazyLock,
};

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "token_fallback")]
    pub token: String,
    pub intents: Intents,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub shards: Option<u64>,
    #[serde(default)]
    pub shard_start: Option<u64>,
    #[serde(default)]
    pub shard_end: Option<u64>,
    #[serde(default)]
    pub activity: Option<Activity>,
    #[serde(default = "default_status")]
    pub status: Status,
    #[serde(default = "default_backpressure")]
    pub backpressure: usize,
    #[serde(default)]
    pub twilight_http_proxy: Option<String>,
    pub externally_accessible_url: String,
    #[serde(default)]
    pub cache: Cache,
}

#[derive(Deserialize, Clone)]
pub struct Cache {
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

impl Default for Cache {
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

impl From<Cache> for EventTypeFlags {
    fn from(cache: Cache) -> EventTypeFlags {
        let mut flags = EventTypeFlags::GUILD_CREATE
            | EventTypeFlags::GUILD_DELETE
            | EventTypeFlags::GUILD_UPDATE
            | EventTypeFlags::READY
            | EventTypeFlags::GATEWAY_INVALIDATE_SESSION;

        if cache.members || cache.current_member {
            flags |= EventTypeFlags::MEMBER_ADD
                | EventTypeFlags::MEMBER_REMOVE
                | EventTypeFlags::MEMBER_UPDATE;
        }

        if cache.roles {
            flags |= EventTypeFlags::ROLE_CREATE
                | EventTypeFlags::ROLE_DELETE
                | EventTypeFlags::ROLE_UPDATE;
        }

        if cache.channels {
            flags |= EventTypeFlags::CHANNEL_CREATE
                | EventTypeFlags::CHANNEL_DELETE
                | EventTypeFlags::CHANNEL_UPDATE
                | EventTypeFlags::THREAD_CREATE
                | EventTypeFlags::THREAD_DELETE
                | EventTypeFlags::THREAD_LIST_SYNC
                | EventTypeFlags::THREAD_UPDATE;
        }

        if cache.presences {
            flags |= EventTypeFlags::PRESENCE_UPDATE;
        }

        if cache.emojis {
            flags |= EventTypeFlags::GUILD_EMOJIS_UPDATE;
        }

        if cache.stage_instances {
            flags |= EventTypeFlags::STAGE_INSTANCE_CREATE
                | EventTypeFlags::STAGE_INSTANCE_DELETE
                | EventTypeFlags::STAGE_INSTANCE_UPDATE;
        }

        if cache.voice_states {
            flags |= EventTypeFlags::VOICE_STATE_UPDATE | EventTypeFlags::VOICE_SERVER_UPDATE;
        }

        if cache.users {
            flags |= EventTypeFlags::USER_UPDATE;
        }

        flags
    }
}

impl From<Cache> for ResourceType {
    fn from(cache: Cache) -> ResourceType {
        let mut resource_types = ResourceType::GUILD | ResourceType::USER_CURRENT;

        if cache.channels {
            resource_types |= ResourceType::CHANNEL;
        }

        if cache.emojis {
            resource_types |= ResourceType::EMOJI;
        }

        if cache.current_member {
            resource_types |= ResourceType::MEMBER_CURRENT;
        }

        if cache.members {
            resource_types |= ResourceType::MEMBER;
        }

        if cache.presences {
            resource_types |= ResourceType::PRESENCE;
        }

        if cache.roles {
            resource_types |= ResourceType::ROLE;
        }

        if cache.stage_instances {
            resource_types |= ResourceType::STAGE_INSTANCE;
        }

        if cache.stickers {
            resource_types |= ResourceType::STICKER;
        }

        if cache.users {
            resource_types |= ResourceType::USER;
        }

        if cache.voice_states {
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

fn token_fallback() -> String {
    if let Ok(token) = var("TOKEN") {
        token
    } else {
        eprintln!("Config Error: token is not present and TOKEN environment variable is not set");
        exit(1);
    }
}

fn default_status() -> Status {
    Status::Online
}

fn default_backpressure() -> usize {
    100
}

pub enum Error {
    InvalidConfig(JsonError),
    NotFound(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidConfig(s) => s.fmt(f),
            Self::NotFound(s) => f.write_fmt(format_args!("File {s} not found or access denied")),
        }
    }
}

#[cfg(feature = "simd-json")]
pub fn load(path: &str) -> Result<Config, Error> {
    let mut content = read_to_string(path).map_err(|_| Error::NotFound(path.to_string()))?;
    let config = unsafe { simd_json::from_str(&mut content) }.map_err(Error::InvalidConfig)?;

    Ok(config)
}

#[cfg(not(feature = "simd-json"))]
pub fn load(path: &str) -> Result<Config, Error> {
    let content = read_to_string(path).map_err(|_| Error::NotFound(path.to_string()))?;
    let config = serde_json::from_str(&content).map_err(Error::InvalidConfig)?;

    Ok(config)
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    match load("config.json") {
        Ok(config) => config,
        Err(err) => {
            // Avoid panicking
            eprintln!("Config Error: {err}");
            exit(1);
        }
    }
});
