use futures_util::{SinkExt, StreamExt};
use log::{debug, info, trace, warn};
use serde::Serialize;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::unbounded_channel,
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Error, Message},
};
use twilight_gateway::{shard::raw_message::Message as TwilightMessage, Event};
use twilight_model::{
    gateway::{
        event::GatewayEventDeserializer,
        payload::{
            incoming::{GuildCreate, GuildDelete, Ready},
            outgoing::Identify,
        },
        OpCode,
    },
    guild::UnavailableGuild,
};

use std::net::SocketAddr;

use crate::state::State;

const HELLO: &str = r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250}}"#;
const HEARTBEAT_ACK: &str = r#"{"t":null,"s":null,"op":11,"d":null}"#;
const INVALID_SESSION: &str = r#"{"t":null,"s":null,"op":9,"d":false}"#;

#[derive(Serialize)]
struct ReadyPayload {
    d: Ready,
    op: OpCode,
    t: String,
    s: usize,
}

#[derive(Serialize)]
struct GuildCreatePayload {
    d: GuildCreate,
    op: OpCode,
    t: String,
    s: usize,
}

#[derive(Serialize)]
struct GuildDeletePayload {
    d: GuildDelete,
    op: OpCode,
    t: String,
    s: usize,
}

async fn handle_client(stream: TcpStream, state: State) -> Result<(), Error> {
    let stream = accept_async(stream).await?;
    let (mut sink, mut stream) = stream.split();

    // Write all messages from a queue to the sink
    let (stream_writer, mut stream_receiver) = unbounded_channel();

    let sink_task = tokio::spawn(async move {
        while let Some(msg) = stream_receiver.recv().await {
            trace!("Sending {:?}", msg);
            sink.send(msg).await?;
        }

        Ok::<(), Error>(())
    });

    // Set up a task that will dump all the messages from the shard to the client
    let (shutdown_reader, mut shutdown_reader_recv) = unbounded_channel();
    let (shard_id_tx, mut shard_id_rx) = unbounded_channel();
    let (shard_send_tx, mut shard_send_rx) = unbounded_channel();
    let state_copy = state.clone();
    let stream_writer_copy = stream_writer.clone();

    tokio::spawn(async move {
        let shard_id = shard_id_rx.recv().await.unwrap();
        // TODO: Shard could already be connected
        let (_, mut shard_status) = state_copy.shards.remove(&shard_id).unwrap();

        debug!("Starting to send events to client");

        if !shard_status.first_time_used {
            let mut seq = 1;

            let unavailable_guilds: Vec<UnavailableGuild> = shard_status
                .guilds
                .iter()
                .map(|(_, guild)| UnavailableGuild {
                    id: guild.id,
                    unavailable: true, // For some reason Discord hardcodes this to true
                })
                .collect();
            // TODO: Should be safe but check
            let mut ready = shard_status.ready.clone().unwrap();
            ready.guilds = unavailable_guilds;
            let ready_payload = ReadyPayload {
                d: ready,
                op: OpCode::Event,
                t: String::from("READY"),
                s: seq,
            };

            if let Ok(serialized) = simd_json::to_string(&ready_payload) {
                debug!("Sending newly created READY");
                let _ = stream_writer_copy.send(Message::Text(serialized));
            };

            for guild_create in shard_status.guilds.iter().filter_map(|(_, guild)| {
                if guild.unavailable {
                    None
                } else {
                    Some(GuildCreate(guild.clone()))
                }
            }) {
                seq += 1;

                let guild_create_payload = GuildCreatePayload {
                    d: guild_create,
                    op: OpCode::Event,
                    t: String::from("GUILD_CREATE"),
                    s: seq,
                };

                if let Ok(serialized) = simd_json::to_string(&guild_create_payload) {
                    trace!("Sending newly created GUILD_CREATE");
                    let _ = stream_writer_copy.send(Message::Text(serialized));
                };
            }

            for guild_delete in shard_status.guilds.iter().filter_map(|(_, guild)| {
                if guild.unavailable {
                    Some(GuildDelete {
                        id: guild.id,
                        unavailable: guild.unavailable,
                    })
                } else {
                    None
                }
            }) {
                seq += 1;

                let guild_delete_payload = GuildDeletePayload {
                    d: guild_delete,
                    op: OpCode::Event,
                    t: String::from("GULD_DELETE"),
                    s: seq,
                };

                if let Ok(serialized) = simd_json::to_string(&guild_delete_payload) {
                    trace!("Sending newly created GUILD_DELETE");
                    let _ = stream_writer_copy.send(Message::Text(serialized));
                };
            }
        }

        loop {
            tokio::select! {
                Some(event) = shard_status.events.next() => {
                    if let Event::ShardPayload(body) = event {
                        let payload = unsafe { String::from_utf8_unchecked(body.bytes) };
                        // The event is always valid
                        let deserializer = GatewayEventDeserializer::from_json(&payload).unwrap();

                        if deserializer.op() == 0 {
                            trace!("Sending payload to client: {:?}", payload);
                            let _ = stream_writer_copy.send(Message::Text(payload));
                        }
                    } else if let Event::Ready(mut ready) = event {
                        ready.guilds.clear();
                        shard_status.ready = Some(*ready);
                    } else if let Event::GuildCreate(guild_create) = event {
                        shard_status.guilds.insert(guild_create.0.id, guild_create.0);
                    } else if let Event::GuildDelete(guild_delete) = event {
                        shard_status.guilds.remove(&guild_delete.id);
                    } else if let Event::GuildUpdate(guild_update) = event {
                        let update_guild = guild_update.0;
                        let guild = shard_status.guilds.get_mut(&update_guild.id);
                        if let Some(guild) = guild {
                            // https://github.com/twilight-rs/twilight/blob/next/cache/in-memory/src/event/guild.rs#L181
                            guild.afk_channel_id = update_guild.afk_channel_id;
                            guild.afk_timeout = update_guild.afk_timeout;
                            guild.banner = update_guild.banner.clone();
                            guild.default_message_notifications = update_guild.default_message_notifications;
                            guild.description = update_guild.description.clone();
                            guild.features = update_guild.features.clone();
                            guild.icon = update_guild.icon.clone();
                            guild.max_members = update_guild.max_members;
                            guild.max_presences = Some(update_guild.max_presences.unwrap_or(25000));
                            guild.mfa_level = update_guild.mfa_level;
                            guild.name = update_guild.name.clone();
                            guild.nsfw_level = update_guild.nsfw_level;
                            guild.owner = update_guild.owner;
                            guild.owner_id = update_guild.owner_id;
                            guild.permissions = update_guild.permissions;
                            guild.preferred_locale = update_guild.preferred_locale.clone();
                            guild.premium_tier = update_guild.premium_tier;
                            guild
                                .premium_subscription_count
                                .replace(update_guild.premium_subscription_count.unwrap_or_default());
                            guild.splash = update_guild.splash.clone();
                            guild.system_channel_id = update_guild.system_channel_id;
                            guild.verification_level = update_guild.verification_level;
                            guild.vanity_url_code = update_guild.vanity_url_code.clone();
                            guild.widget_channel_id = update_guild.widget_channel_id;
                            guild.widget_enabled = update_guild.widget_enabled;
                        }
                    }
                },
                Some(command) = shard_send_rx.recv() => {
                    // Has to be done here because else shard would be moved
                    let _ = shard_status.shard.send(command).await;
                },
                _ = shutdown_reader_recv.recv() => {
                    break;
                }
            };
        }

        debug!("Event stream ended or shutdown requested");

        shard_status.first_time_used = false;

        state_copy.shards.insert(shard_id, shard_status);
    });

    let _ = stream_writer.send(Message::Text(HELLO.to_string()));

    while let Some(Ok(msg)) = stream.next().await {
        let data = msg.into_data();
        let mut payload = unsafe { String::from_utf8_unchecked(data) };

        let deserializer = match GatewayEventDeserializer::from_json(&payload) {
            Some(deserializer) => deserializer,
            None => continue,
        };

        match deserializer.op() {
            1 => {
                trace!("Sending heartbeat ACK");
                let _ = stream_writer.send(Message::Text(HEARTBEAT_ACK.to_string()));
            }
            2 => {
                debug!("Client is identifying");

                let identify: Identify = match simd_json::from_str(&mut payload) {
                    Ok(identify) => identify,
                    Err(_) => continue,
                };

                let (shard_id, shard_count) = if let Some(shard_data) = identify.d.shard {
                    (shard_data[0], shard_data[1])
                } else {
                    warn!("Client sent identify without shard ID, disconnecting");
                    break;
                };

                if shard_count != state.shard_count {
                    warn!("Shard count from client identify mismatched, disconnecting");
                    break;
                }

                trace!("Shard ID is {:?}", shard_id);

                let _ = shard_id_tx.send(shard_id);
            }
            6 => {
                debug!("Client is resuming: {:?}", payload);
                // TODO: Keep track of session IDs and choose one that we have active
                // This would be unnecessary if people forked their clients though
                // For now, send an invalid session so they use identify instead
                let _ = stream_writer.send(Message::text(INVALID_SESSION.to_string()));
            }
            _ => {
                trace!("Sending {:?} to Discord directly", payload);
                let _ = shard_send_tx.send(TwilightMessage::Text(payload));
            }
        }
    }

    sink_task.abort();
    let _ = shutdown_reader.send(());

    Ok(())
}

pub async fn run_server(port: u16, state: State) -> Result<(), Error> {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = TcpListener::bind(addr).await?;

    info!("Listening on {}", addr);

    while let Ok((stream, remote_addr)) = listener.accept().await {
        info!("New connection from {}", remote_addr);

        tokio::spawn(handle_client(stream, state.clone()));
    }

    Ok(())
}
