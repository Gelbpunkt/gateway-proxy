use futures_util::StreamExt;
use log::trace;
use simd_json::Mutable;
use tokio::{sync::broadcast, time::interval};
use twilight_gateway::{shard::Events, Event};

use std::{ops::Range, sync::Arc, time::Duration};

use crate::{deserializer::GatewayEventDeserializer, model::Ready, state::ShardStatus};

/// Payload string and index of the sequence number
pub type BroadcastMessage = (String, Option<(u64, Range<usize>)>);

pub async fn dispatch_events(
    mut events: Events,
    shard_status: Arc<ShardStatus>,
    shard_id: u64,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
) {
    while let Some(event) = events.next().await {
        match event {
            Event::ShardPayload(body) => {
                let mut payload = unsafe { String::from_utf8_unchecked(body.bytes) };
                // The event is always valid
                let deserializer = GatewayEventDeserializer::from_json(&payload).unwrap();
                let (op, sequence, event_type) = deserializer.into_parts();

                if let Some((event_name, _)) = event_type {
                    metrics::increment_counter!("shard_events", "shard" => shard_id.to_string(), "event_type" => event_name.to_string());

                    if event_name == "READY" {
                        // Use the raw JSON from READY to create a new blank READY
                        let mut ready: Ready = simd_json::from_str(&mut payload).unwrap();

                        // Clear the guilds
                        if let Some(guilds) = ready.d.get_mut("guilds") {
                            if let Some(arr) = guilds.as_array_mut() {
                                arr.clear();
                            }
                        }

                        // We don't care if it was already set
                        // since this data is timeless
                        let _res = shard_status.ready.set(ready.d);
                        shard_status.ready_set.notify_waiters();

                        continue;
                    } else if event_name == "RESUMED" {
                        continue;
                    }
                }

                // We only want to relay dispatchable events, not RESUMEs and not READY
                // because we fake a READY event
                if op.0 == 0 {
                    trace!("Sending payload to clients: {:?}", payload);
                    let _res = broadcast_tx.send((payload, sequence));
                }
            }
            Event::GuildCreate(guild_create) => {
                shard_status.guilds.insert(guild_create.0);
            }
            Event::GuildDelete(guild_delete) => {
                shard_status.guilds.remove(guild_delete.id);
            }
            Event::GuildUpdate(guild_update) => {
                shard_status.guilds.update(guild_update.0);
            }
            _ => {}
        }
    }
}

pub async fn shard_latency(shard_status: Arc<ShardStatus>) {
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        if let Ok(info) = shard_status.shard.info() {
            let latency = info.latency().recent().get(0);
            if let Some(latency) = latency {
                metrics::histogram!("shard_latency", latency.as_secs_f64(), "shard" => info.id().to_string());
            }
        }
    }
}
