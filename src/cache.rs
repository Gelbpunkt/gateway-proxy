use dashmap::DashMap;
use halfbrown::hashmap;
use serde::Serialize;
use simd_json::OwnedValue;
use twilight_cache_inmemory::{InMemoryCache, UpdateCache};
use twilight_gateway::{Event as GatewayEvent, Intents};
use twilight_model::{
    channel::{message::Sticker, GuildChannel, StageInstance},
    gateway::{
        payload::incoming::{GuildCreate, GuildDelete, VoiceServerUpdate},
        presence::{Presence, UserOrId},
        OpCode,
    },
    guild::{Emoji, Guild, Member, Role},
    id::{
        marker::{ChannelMarker, GuildMarker},
        Id,
    },
    voice::VoiceState,
};

use std::sync::Arc;

use crate::{config::CONFIG, model::JsonObject};

#[derive(Serialize)]
pub struct Payload {
    d: Event,
    op: OpCode,
    t: String,
    s: usize,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum Event {
    Ready(JsonObject),
    GuildCreate(GuildCreate),
    GuildDelete(GuildDelete),
    VoiceServerUpdate(VoiceServerUpdate),
    VoiceStateUpdate(VoiceState),
}

pub struct VoiceCache(
    DashMap<Id<GuildMarker>, Vec<Event>>,
    Arc<InMemoryCache>,
    u64,
);

impl VoiceCache {
    pub fn new(cache: Arc<InMemoryCache>, shard_id: u64) -> Self {
        Self(DashMap::new(), cache, shard_id)
    }

    pub fn is_in_channel(&self, guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> bool {
        if let Some(payloads) = self.0.get(&guild_id) {
            for payload in payloads.iter() {
                if let Event::VoiceStateUpdate(state) = payload {
                    return state.channel_id.contains(&channel_id);
                }
            }
        }

        false
    }

    pub fn get_payloads(&self, guild_id: Id<GuildMarker>, sequence: &mut usize) -> Vec<Payload> {
        if let Some(events) = self.0.get(&guild_id) {
            events
                .iter()
                .map(|event| {
                    *sequence += 1;

                    Payload {
                        d: event.clone(),
                        op: OpCode::Event,
                        t: if let Event::VoiceServerUpdate(_) = event {
                            String::from("VOICE_SERVER_UPDATE")
                        } else {
                            String::from("VOICE_STATE_UPDATE")
                        },
                        s: *sequence,
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn clear(&self) {
        self.0.clear();
    }

    pub fn disconnect(&self, guild_id: Id<GuildMarker>) {
        self.0.remove(&guild_id);
    }

    pub fn update(&self, value: &GatewayEvent) {
        match value {
            GatewayEvent::VoiceServerUpdate(voice_server_update) => {
                if let Some(guild_id) = voice_server_update.guild_id {
                    // This is always the second event
                    if let Some(mut payloads) = self.0.get_mut(&guild_id) {
                        payloads.push(Event::VoiceServerUpdate(voice_server_update.clone()));
                    }
                }
            }
            GatewayEvent::VoiceStateUpdate(voice_state_update) => {
                if let Some(me) = self.1.current_user() {
                    if me.id == voice_state_update.0.user_id {
                        if let Some(guild_id) = voice_state_update.0.guild_id {
                            // This is always the first event
                            if voice_state_update.0.channel_id.is_some() {
                                self.0.insert(
                                    guild_id,
                                    vec![Event::VoiceStateUpdate(voice_state_update.0.clone())],
                                );
                            } else {
                                // Client disconnected
                                self.0.remove(&guild_id);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct GuildCache(Arc<InMemoryCache>, u64);

impl GuildCache {
    pub fn new(cache: Arc<InMemoryCache>, shard_id: u64) -> Self {
        Self(cache, shard_id)
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

    fn channels_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<GuildChannel> {
        self.0
            .guild_channels(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .filter_map(|channel_id| {
                        let channel = self.0.guild_channel(*channel_id).unwrap();

                        if !matches!(
                            channel.value().resource(),
                            GuildChannel::PrivateThread(_) | GuildChannel::PublicThread(_)
                        ) {
                            Some(channel.value().resource().clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn presences_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<Presence> {
        self.0
            .guild_presences(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|user_id| {
                        let presence = self.0.presence(guild_id, *user_id).unwrap();

                        Presence {
                            activities: presence.activities().to_vec(),
                            client_status: presence.client_status().clone(),
                            guild_id: presence.guild_id(),
                            status: presence.status(),
                            user: UserOrId::UserId {
                                id: presence.user_id(),
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn emojis_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<Emoji> {
        self.0
            .guild_emojis(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|emoji_id| {
                        let emoji = self.0.emoji(*emoji_id).unwrap();

                        Emoji {
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
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn members_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<Member> {
        self.0
            .guild_members(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|user_id| {
                        let member = self.0.member(guild_id, *user_id).unwrap();

                        Member {
                            avatar: member.avatar(),
                            communication_disabled_until: member.communication_disabled_until(),
                            deaf: member.deaf().unwrap_or_default(),
                            guild_id: member.guild_id(),
                            joined_at: member.joined_at(),
                            mute: member.mute().unwrap_or_default(),
                            nick: member.nick().map(ToString::to_string),
                            pending: member.pending(),
                            premium_since: member.premium_since(),
                            roles: member.roles().to_vec(),
                            user: self.0.user(member.user_id()).unwrap().value().clone(), // FIX?
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn roles_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<Role> {
        self.0
            .guild_roles(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|role_id| self.0.role(*role_id).unwrap().value().resource().clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn stage_instances_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<StageInstance> {
        self.0
            .guild_stage_instances(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|stage_id| {
                        self.0
                            .stage_instance(*stage_id)
                            .unwrap()
                            .value()
                            .resource()
                            .clone()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn stickers_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<Sticker> {
        self.0
            .guild_stickers(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|sticker_id| {
                        let sticker = self.0.sticker(*sticker_id).unwrap();

                        Sticker {
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
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn voice_states_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<VoiceState> {
        self.0
            .guild_voice_states(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .map(|user_id| {
                        self.0
                            .voice_state(*user_id, guild_id)
                            .unwrap()
                            .value()
                            .clone()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn threads_in_guild(&self, guild_id: Id<GuildMarker>) -> Vec<GuildChannel> {
        self.0
            .guild_channels(guild_id)
            .map(|reference| {
                reference
                    .iter()
                    .filter_map(|channel_id| {
                        let channel = self.0.guild_channel(*channel_id).unwrap();

                        if matches!(
                            channel.value().resource(),
                            GuildChannel::PrivateThread(_) | GuildChannel::PublicThread(_)
                        ) {
                            Some(channel.value().resource().clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap()
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
                    banner: guild.banner().map(ToOwned::to_owned),
                    approximate_presence_count,
                    channels: guild_channels,
                    default_message_notifications: guild.default_message_notifications(),
                    description: guild.description().map(ToString::to_string),
                    discovery_splash: guild.discovery_splash().map(ToOwned::to_owned),
                    emojis,
                    explicit_content_filter: guild.explicit_content_filter(),
                    features: guild.features().map(ToString::to_string).collect(),
                    icon: guild.icon().map(ToOwned::to_owned),
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
                    premium_progress_bar_enabled: guild.premium_progress_bar_enabled(),
                    premium_subscription_count: guild.premium_subscription_count(),
                    premium_tier: guild.premium_tier(),
                    presences,
                    roles,
                    rules_channel_id: guild.rules_channel_id(),
                    splash: guild.splash().map(ToOwned::to_owned),
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
