use dashmap::DashMap;
use halfbrown::HashMap;
use serde::Serialize;
use simd_json::OwnedValue;
use twilight_model::{
    gateway::{
        payload::incoming::{GuildCreate, GuildDelete},
        OpCode,
    },
    guild::{Guild, PartialGuild},
    id::GuildId,
};

use crate::model::JsonObject;

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
    Ready(JsonObject),
    GuildCreate(GuildCreate),
    GuildDelete(GuildDelete),
}

pub struct GuildCache(DashMap<GuildId, Guild>, u64);

impl GuildCache {
    pub fn new(shard_id: u64) -> Self {
        Self(DashMap::new(), shard_id)
    }

    pub fn insert(&self, guild_create: Guild) {
        metrics::increment_gauge!("guild_cache_size", 1.0, "shard" => self.1.to_string());

        self.0.insert(guild_create.id, guild_create);
    }

    pub fn remove(&self, guild_id: GuildId) {
        metrics::decrement_gauge!("guild_cache_size", 1.0, "shard" => self.1.to_string());

        self.0.remove(&guild_id);
    }

    pub fn update(&self, guild_update: PartialGuild) {
        let guild = self.0.get_mut(&guild_update.id);
        if let Some(mut guild) = guild {
            // https://github.com/twilight-rs/twilight/blob/next/cache/in-memory/src/event/guild.rs#L181
            guild.afk_channel_id = guild_update.afk_channel_id;
            guild.afk_timeout = guild_update.afk_timeout;
            guild.banner = guild_update.banner;
            guild.default_message_notifications = guild_update.default_message_notifications;
            guild.description = guild_update.description;
            guild.features = guild_update.features;
            guild.icon = guild_update.icon;
            guild.max_members = guild_update.max_members;
            guild.max_presences = Some(guild_update.max_presences.unwrap_or(25000));
            guild.mfa_level = guild_update.mfa_level;
            guild.name = guild_update.name;
            guild.nsfw_level = guild_update.nsfw_level;
            guild.owner = guild_update.owner;
            guild.owner_id = guild_update.owner_id;
            guild.permissions = guild_update.permissions;
            guild.preferred_locale = guild_update.preferred_locale;
            guild.premium_tier = guild_update.premium_tier;
            guild
                .premium_subscription_count
                .replace(guild_update.premium_subscription_count.unwrap_or_default());
            guild.splash = guild_update.splash;
            guild.system_channel_id = guild_update.system_channel_id;
            guild.verification_level = guild_update.verification_level;
            guild.vanity_url_code = guild_update.vanity_url_code;
            guild.widget_channel_id = guild_update.widget_channel_id;
            guild.widget_enabled = guild_update.widget_enabled;
        }
    }

    pub fn get_ready_payload(&self, mut ready: JsonObject, sequence: &mut usize) -> Payload {
        *sequence += 1;

        let unavailable_guilds = self
            .0
            .iter()
            .map(|guild| {
                let mut value = HashMap::with_capacity(2);
                value.insert(String::from("id"), OwnedValue::from(guild.id.to_string()));
                value.insert(String::from("unavailable"), OwnedValue::from(true));

                OwnedValue::from(value)
            })
            .collect();

        ready.insert(
            String::from("guilds"),
            OwnedValue::Array(unavailable_guilds),
        );

        Payload {
            d: Event::Ready(ready),
            op: OpCode::Event,
            t: String::from("READY"),
            s: *sequence,
        }
    }

    pub fn get_guild_payloads<'a>(
        &'a self,
        sequence: &'a mut usize,
    ) -> impl Iterator<Item = Payload> + 'a {
        self.0.iter().map(move |guild| {
            *sequence += 1;

            if guild.unavailable {
                let guild_delete = GuildDelete {
                    id: guild.id,
                    unavailable: guild.unavailable,
                };

                Payload {
                    d: Event::GuildDelete(guild_delete),
                    op: OpCode::Event,
                    t: String::from("GUILD_DELETE"),
                    s: *sequence,
                }
            } else {
                let guild_create = GuildCreate(guild.clone());

                Payload {
                    d: Event::GuildCreate(guild_create),
                    op: OpCode::Event,
                    t: String::from("GUILD_CREATE"),
                    s: *sequence,
                }
            }
        })
    }
}
