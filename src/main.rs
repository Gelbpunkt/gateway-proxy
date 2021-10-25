use dashmap::DashMap;
use log::{debug, error};
use twilight_gateway::{EventTypeFlags, Shard};
use twilight_gateway_queue::{LargeBotQueue, Queue};
use twilight_http::Client;

use std::{env::set_var, error::Error, process::exit, sync::Arc};

mod cache;
mod config;
mod server;
mod state;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = match config::load("config.json") {
        Ok(config) => config,
        Err(err) => {
            // Avoid panicking
            eprintln!("Config Error: {}", err);
            exit(1);
        }
    };

    set_var("RUST_LOG", config.log_level);
    env_logger::builder().format_timestamp_millis().init();

    // Set up a HTTPClient
    let client = Arc::new(Client::new(config.token.clone()));

    // Check total shards required
    let gateway = client.gateway().authed().exec().await?.model().await?;

    let shard_count = config.shards.unwrap_or(gateway.shards);

    // Set up a queue for the shards
    let queue: Arc<dyn Queue> = Arc::new(
        LargeBotQueue::new(gateway.session_start_limit.max_concurrency as usize, client).await,
    );

    // Create all shards
    let shards: state::Shards = DashMap::with_capacity(shard_count as usize);

    for shard_id in 0..shard_count {
        let (shard, events) = Shard::builder(config.token.clone(), config.intents)
            .queue(queue.clone())
            .shard(shard_id, shard_count)?
            .gateway_url(Some(gateway.url.clone()))
            .event_types(
                EventTypeFlags::SHARD_PAYLOAD
                    | EventTypeFlags::READY
                    | EventTypeFlags::GUILD_CREATE
                    | EventTypeFlags::GUILD_DELETE
                    | EventTypeFlags::GUILD_UPDATE,
            )
            .build();

        shard.start().await?;

        let shard_status = state::ShardStatus {
            shard,
            events,
            first_time_used: true,
            ready: None,
            guilds: cache::GuildCache::new(),
        };

        shards.insert(shard_id, shard_status);

        debug!("Created shard {} of {} total", shard_id, shard_count);
    }

    let state = Arc::new(state::StateInner {
        shards,
        shard_count,
    });

    if let Err(e) = server::run_server(config.port, state).await {
        error!("{}", e);
    };

    Ok(())
}
