use futures_util::StreamExt;
use log::trace;
use tokio::sync::broadcast;
use twilight_gateway::{shard::Events, Event};
use twilight_model::gateway::event::GatewayEventDeserializer;

use std::sync::Arc;

use crate::state::ShardStatus;

pub async fn dispatch_events(
    mut events: Events,
    shard_status: Arc<ShardStatus>,
    broadcast_tx: broadcast::Sender<String>,
) {
    while let Some(event) = events.next().await {
        if let Event::ShardPayload(body) = event {
            let payload = unsafe { String::from_utf8_unchecked(body.bytes) };
            // The event is always valid
            let deserializer = GatewayEventDeserializer::from_json(&payload).unwrap();

            // We only want to relay dispatchable events, not RESUMEs and not READY
            // because we fake a READY event
            if deserializer.op() == 0
                && !deserializer.event_type_ref().contains(&"RESUMED")
                && !deserializer.event_type_ref().contains(&"READY")
            {
                trace!("Sending payload to clients: {:?}", payload);
                let _res = broadcast_tx.send(payload);
            }
        } else if let Event::Ready(mut ready) = event {
            ready.guilds.clear();
            // We don't care if it was already set
            // since this data is timeless
            let _res = shard_status.ready.set(*ready);
            shard_status.ready_set.notify_waiters();
        } else if let Event::GuildCreate(guild_create) = event {
            shard_status.guilds.insert(guild_create.0);
        } else if let Event::GuildDelete(guild_delete) = event {
            shard_status.guilds.remove(guild_delete.id);
        } else if let Event::GuildUpdate(guild_update) = event {
            shard_status.guilds.update(guild_update.0);
        }
    }
}
