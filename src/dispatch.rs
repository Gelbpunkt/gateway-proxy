use futures_util::StreamExt;
use simd_json::Mutable;
use tokio::{sync::broadcast, time::interval};
use tracing::{debug, trace};
use twilight_gateway::{
    shard::{Events, Stage},
    Event,
};

use std::{sync::Arc, time::Duration};

use crate::{
    deserializer::{EventTypeInfo, GatewayEvent, SequenceInfo},
    model::Ready,
    state::Shard,
};

pub type BroadcastMessage = (String, Option<SequenceInfo>);

pub async fn events(
    mut events: Events,
    shard_state: Arc<Shard>,
    shard_id: u64,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
) {
    // This method only wants to relay events while the shard is in a READY state
    // Therefore, we only put events in the queue while we are connected and READY
    let mut is_ready = false;

    while let Some(event) = events.next().await {
        if let Event::ShardPayload(body) = event {
            let mut payload = unsafe { String::from_utf8_unchecked(body.bytes) };
            // The event is always valid
            let deserializer = GatewayEvent::from_json(&payload).unwrap();
            let (op, sequence, event_type) = deserializer.into_parts();

            if let Some(EventTypeInfo(event_name, _)) = event_type {
                metrics::increment_counter!("gateway_shard_events", "shard" => shard_id.to_string(), "event_type" => event_name.to_string());

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
                    shard_state.ready.set_ready(ready.d);
                    is_ready = true;

                    continue;
                } else if event_name == "RESUMED" {
                    is_ready = true;

                    continue;
                }
            }

            // We only want to relay dispatchable events, not RESUMEs and not READY
            // because we fake a READY event
            if op.0 == 0 {
                trace!(
                    "[Shard {}] Sending payload to clients: {:?}",
                    shard_id,
                    payload
                );

                if is_ready {
                    let _res = broadcast_tx.send((payload, sequence));
                }
            }
        } else if let Event::ShardReconnecting(_) = event {
            debug!("[Shard {}] Reconnecting", shard_id);
            shard_state.ready.set_not_ready();
            is_ready = false;
        } else {
            shard_state.guilds.update(event);
        }
    }
}

pub async fn shard_latency(shard_state: Arc<Shard>) {
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        if let Ok(info) = shard_state.shard.info() {
            // There is no way around this, sadly
            let connection_status = match info.stage() {
                Stage::Connected => 4.0,
                Stage::Disconnected => 0.0,
                Stage::Handshaking => 1.0,
                Stage::Identifying => 2.0,
                Stage::Resuming => 3.0,
                _ => f64::NAN,
            };

            let latency = info
                .latency()
                .recent()
                .get(4)
                .map_or(f64::NAN, Duration::as_secs_f64);

            metrics::histogram!("gateway_shard_latency_histogram", latency, "shard" => info.id().to_string());
            metrics::gauge!(
                "gateway_shard_latency",
                latency,
                "shard" => info.id().to_string()
            );
            metrics::histogram!("gateway_shard_status", connection_status, "shard" => info.id().to_string());
        }
    }
}
