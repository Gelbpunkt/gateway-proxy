use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use serde::Deserialize;
#[cfg(not(feature = "simd-json"))]
use serde_json::Error as JsonError;
#[cfg(feature = "simd-json")]
use simd_json::Error as JsonError;
use tracing_subscriber::{filter::LevelFilter, reload};
use twilight_cache_inmemory::ResourceType;
use twilight_gateway::{EventTypeFlags, Intents};
use twilight_model::gateway::presence::{Activity, Status};

use std::{
    env::var,
    fmt::{Display, Formatter, Result as FmtResult},
    fs::read_to_string,
    process::exit,
    str::FromStr,
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
    pub shards: Option<u32>,
    #[serde(default)]
    pub shard_start: Option<u32>,
    #[serde(default)]
    pub shard_end: Option<u32>,
    #[serde(default)]
    pub activity: Option<Activity>,
    #[serde(default = "default_status")]
    pub status: Status,
    #[serde(default = "default_backpressure")]
    pub backpressure: usize,
    #[serde(default = "default_validate_token")]
    pub validate_token: bool,
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
    pub scheduled_events: bool,
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
            scheduled_events: false,
            stage_instances: false,
            stickers: false,
            users: false,
            voice_states: false,
        }
    }
}

impl From<Cache> for EventTypeFlags {
    fn from(cache: Cache) -> Self {
        let mut flags = Self::GUILD_CREATE
            | Self::GUILD_DELETE
            | Self::GUILD_UPDATE
            | Self::READY
            | Self::GATEWAY_INVALIDATE_SESSION;

        if cache.members || cache.current_member {
            flags |= Self::MEMBER_ADD | Self::MEMBER_REMOVE | Self::MEMBER_UPDATE;
        }

        if cache.roles {
            flags |= Self::ROLE_CREATE | Self::ROLE_DELETE | Self::ROLE_UPDATE;
        }

        if cache.channels {
            flags |= Self::CHANNEL_CREATE
                | Self::CHANNEL_DELETE
                | Self::CHANNEL_UPDATE
                | Self::THREAD_CREATE
                | Self::THREAD_DELETE
                | Self::THREAD_LIST_SYNC
                | Self::THREAD_UPDATE;
        }

        if cache.presences {
            flags |= Self::PRESENCE_UPDATE;
        }

        if cache.emojis {
            flags |= Self::GUILD_EMOJIS_UPDATE;
        }

        if cache.scheduled_events {
            flags |= Self::GUILD_SCHEDULED_EVENTS;
        }

        if cache.stage_instances {
            flags |= Self::STAGE_INSTANCE_CREATE
                | Self::STAGE_INSTANCE_DELETE
                | Self::STAGE_INSTANCE_UPDATE;
        }

        if cache.voice_states {
            flags |= Self::VOICE_STATE_UPDATE | Self::VOICE_SERVER_UPDATE;
        }

        if cache.users {
            flags |= Self::USER_UPDATE;
        }

        flags
    }
}

impl From<Cache> for ResourceType {
    fn from(cache: Cache) -> Self {
        let mut resource_types = Self::GUILD | Self::USER_CURRENT;

        if cache.channels {
            resource_types |= Self::CHANNEL;
        }

        if cache.emojis {
            resource_types |= Self::EMOJI;
        }

        if cache.current_member {
            resource_types |= Self::MEMBER_CURRENT;
        }

        if cache.members {
            resource_types |= Self::MEMBER;
        }

        if cache.presences {
            resource_types |= Self::PRESENCE;
        }

        if cache.roles {
            resource_types |= Self::ROLE;
        }

        if cache.scheduled_events {
            resource_types |= Self::GUILD_SCHEDULED_EVENT;
        }

        if cache.stage_instances {
            resource_types |= Self::STAGE_INSTANCE;
        }

        if cache.stickers {
            resource_types |= Self::STICKER;
        }

        if cache.users {
            resource_types |= Self::USER;
        }

        if cache.voice_states {
            resource_types |= Self::VOICE_STATE;
        }

        resource_types
    }
}

fn default_log_level() -> String {
    String::from("info")
}

const fn default_port() -> u16 {
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

const fn default_status() -> Status {
    Status::Online
}

const fn default_backpressure() -> usize {
    100
}

const fn default_validate_token() -> bool {
    true
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

pub async fn watch_config_changes<S>(reload_handle: reload::Handle<LevelFilter, S>) {
    let Ok(inotify) = Inotify::init() else {
        tracing::error!("Failed to initialize inotify, log-levels cannot be reloaded on the fly");
        return;
    };

    if inotify
        .watches()
        .add("config.json", WatchMask::MODIFY)
        .is_err()
    {
        tracing::error!("Failed to add inotify watch, log-levels cannot be reloaded on the fly");
        return;
    };

    tracing::debug!("Inotify is initialized");

    let buffer = [0u8; 4096];
    // This method never returns Err
    let mut events = inotify.into_event_stream(buffer).unwrap();

    while let Some(Ok(_)) = events.next().await {
        // This currently only supports reloading log-levels
        if let Ok(config) = load("config.json") {
            let _ = reload_handle.modify(|filter| {
                *filter = LevelFilter::from_str(&config.log_level).unwrap_or(LevelFilter::INFO);
            });
            tracing::info!("Config was modified, reloaded log-level");
        } else {
            tracing::error!("Config was modified, but failed to reload");
        }
    }
}
