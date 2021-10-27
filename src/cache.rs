use halfbrown::hashmap;
use serde::Serialize;
use simd_json::OwnedValue;
use twilight_cache_inmemory::{InMemoryCache, UpdateCache};
use twilight_gateway::Intents;
use twilight_model::{
    channel::{message::Sticker, GuildChannel, StageInstance},
    gateway::{
        payload::incoming::{GuildCreate, GuildDelete},
        presence::{Presence, UserOrId},
        OpCode,
    },
    guild::{Emoji, Guild, Member, Role},
    id::GuildId,
    voice::VoiceState,
};

use crate::{config::CONFIG, model::JsonObject};

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

pub struct GuildCache(InMemoryCache, u64);

impl GuildCache {
    pub fn new(shard_id: u64) -> Self {
        let resource_types = CONFIG.cache.clone().into();

        Self(
            InMemoryCache::builder()
                .resource_types(resource_types)
                .build(),
            shard_id,
        )
    }

    pub fn update(&self, value: &impl UpdateCache) {
        self.0.update(value);
    }

    pub fn get_ready_payload(&self, mut ready: JsonObject, sequence: &mut usize) -> Payload {
        *sequence += 1;

        let unavailable_guilds = self
            .0
            .iter()
            .guilds()
            .map(|guild| {
                hashmap! {
                    String::from("id") => guild.id().to_string().into(),
                    String::from("unavailable") => true.into(),
                }
                .into()
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

    fn channels_in_guild(&self, guild_id: GuildId) -> Vec<GuildChannel> {
        self.0
            .iter()
            .guild_channels()
            .filter_map(|channel| {
                if channel.guild_id() == guild_id
                    && !matches!(
                        channel.value().resource(),
                        GuildChannel::PrivateThread(_) | GuildChannel::PublicThread(_)
                    )
                {
                    Some(channel.value().resource().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn presences_in_guild(&self, guild_id: GuildId) -> Vec<Presence> {
        self.0
            .iter()
            .presences()
            .filter_map(|presence| {
                if presence.guild_id() == guild_id {
                    Some(Presence {
                        activities: presence.activities().to_vec(),
                        client_status: presence.client_status().clone(),
                        guild_id: presence.guild_id(),
                        status: presence.status(),
                        user: UserOrId::UserId {
                            id: presence.user_id(),
                        },
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn emojis_in_guild(&self, guild_id: GuildId) -> Vec<Emoji> {
        self.0
            .iter()
            .emojis()
            .filter_map(|emoji| {
                if emoji.guild_id() == guild_id {
                    Some(Emoji {
                        animated: emoji.animated(),
                        available: emoji.available(),
                        id: emoji.id(),
                        managed: emoji.managed(),
                        name: emoji.name().to_string(),
                        require_colons: emoji.require_colons(),
                        roles: emoji.roles().to_vec(),
                        user: emoji
                            .user_id()
                            .and_then(|id| self.0.user(id).map(|user| user.value().clone())),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn members_in_guild(&self, guild_id: GuildId) -> Vec<Member> {
        self.0
            .iter()
            .members()
            .filter_map(|member| {
                if member.guild_id() == guild_id {
                    Some(Member {
                        deaf: member.deaf().unwrap_or_default(),
                        guild_id: member.guild_id(),
                        hoisted_role: None, // TODO
                        joined_at: member.joined_at(),
                        mute: member.mute().unwrap_or_default(),
                        nick: member.nick().map(ToString::to_string),
                        pending: member.pending(),
                        premium_since: member.premium_since(),
                        roles: member.roles().to_vec(),
                        user: self.0.user(member.user_id()).unwrap().value().clone(), // FIX?
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn roles_in_guild(&self, guild_id: GuildId) -> Vec<Role> {
        self.0
            .iter()
            .roles()
            .filter_map(|role| {
                if role.guild_id() == guild_id {
                    Some(role.value().resource().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn stage_instances_in_guild(&self, guild_id: GuildId) -> Vec<StageInstance> {
        self.0
            .iter()
            .stage_instances()
            .filter_map(|stage| {
                if stage.guild_id() == guild_id {
                    Some(stage.value().resource().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn stickers_in_guild(&self, guild_id: GuildId) -> Vec<Sticker> {
        self.0
            .iter()
            .stickers()
            .filter_map(|sticker| {
                if sticker.guild_id() == guild_id {
                    Some(Sticker {
                        available: sticker.available(),
                        description: Some(sticker.description().to_string()),
                        format_type: sticker.format_type(),
                        guild_id: Some(sticker.guild_id()),
                        id: sticker.id(),
                        kind: sticker.kind(),
                        name: sticker.name().to_string(),
                        pack_id: sticker.pack_id(),
                        sort_value: sticker.sort_value(),
                        tags: sticker.tags().to_string(),
                        user: sticker
                            .user_id()
                            .and_then(|id| self.0.user(id).map(|user| user.value().clone())),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn voice_states_in_guild(&self, guild_id: GuildId) -> Vec<VoiceState> {
        self.0
            .iter()
            .voice_states()
            .filter_map(|voice| {
                if voice.key().0 == guild_id {
                    Some(voice.value().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn threads_in_guild(&self, guild_id: GuildId) -> Vec<GuildChannel> {
        self.0
            .iter()
            .guild_channels()
            .filter_map(|channel| {
                if channel.guild_id() == guild_id
                    && matches!(
                        channel.value().resource(),
                        GuildChannel::PrivateThread(_) | GuildChannel::PublicThread(_)
                    )
                {
                    Some(channel.value().resource().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_guild_payloads<'a>(
        &'a self,
        sequence: &'a mut usize,
    ) -> impl Iterator<Item = Payload> + 'a {
        self.0.iter().guilds().map(move |guild| {
            *sequence += 1;

            if guild.unavailable() {
                let guild_delete = GuildDelete {
                    id: guild.id(),
                    unavailable: true,
                };

                Payload {
                    d: Event::GuildDelete(guild_delete),
                    op: OpCode::Event,
                    t: String::from("GUILD_DELETE"),
                    s: *sequence,
                }
            } else {
                let guild_channels = self.channels_in_guild(guild.id());
                let presences = self.presences_in_guild(guild.id());
                let emojis = self.emojis_in_guild(guild.id());
                let members = self.members_in_guild(guild.id());
                let roles = self.roles_in_guild(guild.id());
                let stage_instances = self.stage_instances_in_guild(guild.id());
                let stickers = self.stickers_in_guild(guild.id());
                let voice_states = self.voice_states_in_guild(guild.id());
                let threads = self.threads_in_guild(guild.id());

                let approximate_member_count =
                    if CONFIG.cache.members && CONFIG.intents.contains(Intents::GUILD_MEMBERS) {
                        Some(members.len() as u64)
                    } else {
                        None
                    };

                let approximate_presence_count = if CONFIG.cache.presences
                    && CONFIG.intents.contains(Intents::GUILD_PRESENCES)
                {
                    Some(presences.len() as u64)
                } else {
                    None
                };

                let new_guild = Guild {
                    afk_channel_id: guild.afk_channel_id(),
                    afk_timeout: guild.afk_timeout(),
                    application_id: guild.application_id(),
                    approximate_member_count,
                    banner: guild.banner().map(ToString::to_string),
                    approximate_presence_count,
                    channels: guild_channels,
                    default_message_notifications: guild.default_message_notifications(),
                    description: guild.description().map(ToString::to_string),
                    discovery_splash: guild.discovery_splash().map(ToString::to_string),
                    emojis,
                    explicit_content_filter: guild.explicit_content_filter(),
                    features: guild.features().map(ToString::to_string).collect(),
                    icon: guild.icon().map(ToString::to_string),
                    id: guild.id(),
                    joined_at: guild.joined_at(),
                    large: guild.large(),
                    max_members: guild.max_members(),
                    max_presences: guild.max_presences(),
                    max_video_channel_users: None, // Not in the cache model
                    member_count: guild.member_count(),
                    members,
                    mfa_level: guild.mfa_level(),
                    name: guild.name().to_string(),
                    nsfw_level: guild.nsfw_level(),
                    owner_id: guild.owner_id(),
                    owner: guild.owner(),
                    permissions: guild.permissions(),
                    preferred_locale: guild.preferred_locale().to_string(),
                    premium_subscription_count: guild.premium_subscription_count(),
                    premium_tier: guild.premium_tier(),
                    presences,
                    roles,
                    rules_channel_id: guild.rules_channel_id(),
                    splash: guild.splash().map(ToString::to_string),
                    stage_instances,
                    stickers,
                    system_channel_flags: guild.system_channel_flags(),
                    system_channel_id: guild.system_channel_id(),
                    threads,
                    unavailable: false,
                    vanity_url_code: guild.vanity_url_code().map(ToString::to_string),
                    verification_level: guild.verification_level(),
                    voice_states,
                    widget_channel_id: guild.widget_channel_id(),
                    widget_enabled: guild.widget_enabled(),
                };

                let guild_create = GuildCreate(new_guild);

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
