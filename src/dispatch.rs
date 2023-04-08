use itoa::Buffer;
#[cfg(feature = "simd-json")]
use simd_json::Mutable;
use tokio::{sync::broadcast, time::Instant};
use tracing::{debug, trace};
use twilight_gateway::{parse, ConnectionStatus, Event, EventTypeFlags, Message, Shard};
use twilight_model::gateway::event::GatewayEvent as TwilightGatewayEvent;

use std::{sync::Arc, time::Duration};

use crate::{
    config::CONFIG,
    deserializer::{EventTypeInfo, GatewayEvent, SequenceInfo},
    model::Ready,
    state::Shard as ShardState,
};

pub type BroadcastMessage = (String, Option<SequenceInfo>);

const TEN_SECONDS: Duration = Duration::from_secs(10);

pub async fn events(
    mut shard: Shard,
    shard_state: Arc<ShardState>,
    shard_id: u32,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
) {
    // This method only wants to relay events while the shard is in a READY state
    // Therefore, we only put events in the queue while we are connected and READY
    let mut is_ready = false;

    let mut buffer = Buffer::new();
    let shard_id_str = buffer.format(shard_id).to_owned();

    let mut last_metrics_update = Instant::now();

    let event_type_flags: EventTypeFlags = CONFIG.cache.clone().into();

    loop {
        // Update metrics if the last update was more than 10s ago
        let now = Instant::now();

        if now.duration_since(last_metrics_update) > TEN_SECONDS {
            let latencies = shard.latency().recent();
            let info = shard.status();
            update_shard_statistics(&shard_id_str, &shard_state, info, latencies);
            last_metrics_update = now;
        }

        let msg = match shard.next_message().await {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("Error receiving message: {e}");

                if e.is_fatal() {
                    break;
                }

                continue;
            }
        };

        // NOTE: payload cannot be modified because we have to do optional event parsing
        // later. Don't use simd_json::from_str on it because that will make the data useless.
        // Instead, clone it before mutating.
        if let Message::Text(payload) = msg {
            let Some(event ) = GatewayEvent::from_json(&payload) else {
                tracing::error!("Failed to deserialize gateway event");
                continue;
            };

            let (op, sequence, event_type) = event.into_parts();

            if let Some(EventTypeInfo(event_name, _)) = event_type {
                metrics::increment_counter!("gateway_shard_events", "shard" => shard_id_str.clone(), "event_type" => event_name.to_owned());

                if event_name == "READY" {
                    // Use the raw JSON from READY to create a new blank READY

                    #[cfg(feature = "simd-json")]
                    let mut ready: Ready =
                        unsafe { simd_json::from_str(&mut payload.clone()).unwrap() };
                    #[cfg(not(feature = "simd-json"))]
                    let mut ready: Ready = serde_json::from_str(&payload).unwrap();

                    // Clear the guilds
                    if let Some(guilds) = ready.d.get_mut("guilds") {
                        if let Some(arr) = guilds.as_array_mut() {
                            arr.clear();
                        }
                    }

                    // Override resume_gateway_url with the external URI of the proxy
                    ready.d.insert(
                        String::from("resume_gateway_url"),
                        CONFIG.externally_accessible_url.clone().into(),
                    );

                    // We don't care if it was already set
                    // since this data is timeless
                    shard_state.ready.set_ready(ready.d);
                    is_ready = true;
                } else if event_name == "RESUMED" {
                    is_ready = true;
                } else if op.0 == 0 && is_ready {
                    // We only want to relay dispatchable events, not RESUMEs and not READY
                    // because we fake a READY event
                    let payload_copy = payload.clone();
                    trace!("[Shard {shard_id}] Sending payload to clients: {payload_copy:?}",);

                    let _res = broadcast_tx.send((payload_copy, sequence));
                }
            }

            if let Ok(Some(event)) = parse(payload, event_type_flags) {
                match event {
                    TwilightGatewayEvent::Dispatch(_, event) => {
                        shard_state.guilds.update(Event::from(event));
                    }
                    TwilightGatewayEvent::InvalidateSession(can_resume) => {
                        debug!("[Shard {shard_id}] Session invalidated, resumable: {can_resume}");
                        if !can_resume {
                            // We can only reset the READY state if we know that we will get a new READY,
                            // which is the case if we can not resume.
                            shard_state.ready.set_not_ready();
                        }
                        // Suspend sending events to clients until READY or RESUMED are received.
                        is_ready = false;
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn update_shard_statistics(
    shard_id: &str,
    shard_state: &Arc<ShardState>,
    connection_status: &ConnectionStatus,
    latencies: &[Duration],
) {
    // There is no way around this, sadly
    let connection_status = match connection_status {
        ConnectionStatus::Connected => 4.0,
        ConnectionStatus::Disconnected { .. } => 1.0,
        ConnectionStatus::Identifying => 2.0,
        ConnectionStatus::Resuming => 3.0,
        ConnectionStatus::FatallyClosed { .. } => 0.0,
    };

    let latency = latencies.first().map_or(f64::NAN, Duration::as_secs_f64);

    metrics::histogram!("gateway_shard_latency_histogram", latency, "shard" => shard_id.to_string());
    metrics::gauge!(
        "gateway_shard_latency",
        latency,
        "shard" => shard_id.to_string()
    );
    metrics::histogram!("gateway_shard_status", connection_status, "shard" => shard_id.to_string());

    let stats = shard_state.guilds.stats();

    metrics::gauge!("gateway_cache_emojis", stats.emojis() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_guilds", stats.guilds() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_members", stats.members() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_presences", stats.presences() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_channels", stats.channels() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_roles", stats.roles() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_unavailable_guilds", stats.unavailable_guilds() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_users", stats.users() as f64, "shard" => shard_id.to_string());
    metrics::gauge!("gateway_cache_voice_states", stats.voice_states() as f64, "shard" => shard_id.to_string());
}
