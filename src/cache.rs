use serde::Serialize;
use twilight_model::{
    gateway::{
        payload::incoming::{GuildCreate, GuildDelete, Ready},
        OpCode,
    },
    guild::{Guild, PartialGuild, UnavailableGuild},
    id::GuildId,
};

use std::collections::HashMap;

#[derive(Serialize)]
pub struct Payload {
    d: Event,
    op: OpCode,
    t: String,
    s: usize,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum Event {
    Ready(Ready),
    GuildCreate(GuildCreate),
    GuildDelete(GuildDelete),
}

pub struct GuildCache(HashMap<GuildId, Guild>);

impl GuildCache {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, guild_create: Guild) {
        self.0.insert(guild_create.id, guild_create);
    }

    pub fn remove(&mut self, guild_id: &GuildId) {
        self.0.remove(guild_id);
    }

    pub fn update(&mut self, guild_update: PartialGuild) {
        let guild = self.0.get_mut(&guild_update.id);
        if let Some(guild) = guild {
            // https://github.com/twilight-rs/twilight/blob/next/cache/in-memory/src/event/guild.rs#L181
            guild.afk_channel_id = guild_update.afk_channel_id;
            guild.afk_timeout = guild_update.afk_timeout;
            guild.banner = guild_update.banner.clone();
            guild.default_message_notifications = guild_update.default_message_notifications;
            guild.description = guild_update.description.clone();
            guild.features = guild_update.features.clone();
            guild.icon = guild_update.icon.clone();
            guild.max_members = guild_update.max_members;
            guild.max_presences = Some(guild_update.max_presences.unwrap_or(25000));
            guild.mfa_level = guild_update.mfa_level;
            guild.name = guild_update.name.clone();
            guild.nsfw_level = guild_update.nsfw_level;
            guild.owner = guild_update.owner;
            guild.owner_id = guild_update.owner_id;
            guild.permissions = guild_update.permissions;
            guild.preferred_locale = guild_update.preferred_locale.clone();
            guild.premium_tier = guild_update.premium_tier;
            guild
                .premium_subscription_count
                .replace(guild_update.premium_subscription_count.unwrap_or_default());
            guild.splash = guild_update.splash.clone();
            guild.system_channel_id = guild_update.system_channel_id;
            guild.verification_level = guild_update.verification_level;
            guild.vanity_url_code = guild_update.vanity_url_code.clone();
            guild.widget_channel_id = guild_update.widget_channel_id;
            guild.widget_enabled = guild_update.widget_enabled;
        }
    }

    pub fn get_ready_payload(&self, mut ready: Ready) -> Payload {
        let unavailable_guilds = self
            .0
            .iter()
            .map(|(_, guild)| UnavailableGuild {
                id: guild.id,
                unavailable: true, // For some reason Discord hardcodes this to true
            })
            .collect();
        ready.guilds = unavailable_guilds;

        Payload {
            d: Event::Ready(ready),
            op: OpCode::Event,
            t: String::from("READY"),
            s: 1,
        }
    }

    pub fn get_guild_payloads(&self) -> impl Iterator<Item = Payload> + '_ {
        self.0.iter().enumerate().map(|(i, (_, guild))| {
            if guild.unavailable {
                let guild_delete = GuildDelete {
                    id: guild.id,
                    unavailable: guild.unavailable,
                };
                let payload = Payload {
                    d: Event::GuildDelete(guild_delete),
                    op: OpCode::Event,
                    t: String::from("GUILD_DELETE"),
                    s: 2 + i,
                };

                payload
            } else {
                let guild_create = GuildCreate(guild.clone());
                let payload = Payload {
                    d: Event::GuildCreate(guild_create),
                    op: OpCode::Event,
                    t: String::from("GUILD_CREATE"),
                    s: 2 + i,
                };

                payload
            }
        })
    }
}
