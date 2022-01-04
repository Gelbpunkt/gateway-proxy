#![feature(option_result_contains, once_cell)]
#![deny(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::large_enum_variant,
    clippy::cast_ptr_alignment,
    clippy::struct_excessive_bools,
    clippy::from_over_into
)]
use libc::{c_int, sighandler_t, signal, SIGINT, SIGTERM};
use metrics_exporter_prometheus::PrometheusBuilder;
use mimalloc::MiMalloc;
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::Shard;
use twilight_gateway_queue::{LargeBotQueue, Queue};
use twilight_http::Client;
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;

use std::{env::set_var, error::Error, sync::Arc};

use crate::config::CONFIG;

mod cache;
mod config;
mod deserializer;
mod dispatch;
mod model;
mod server;
mod state;
mod upgrade;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub extern "C" fn handler(_: c_int) {
    std::process::exit(0);
}

unsafe fn set_os_handlers() {
    signal(SIGINT, handler as extern "C" fn(_) as sighandler_t);
    signal(SIGTERM, handler as extern "C" fn(_) as sighandler_t);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Exit on SIGINT/SIGTERM, used in Docker
    unsafe { set_os_handlers() };

    set_var("RUST_LOG", CONFIG.log_level.clone());

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Set up metrics collection
    let recorder = PrometheusBuilder::new().build();
    let metrics_handle = Arc::new(recorder.handle());
    metrics::set_boxed_recorder(Box::new(recorder)).unwrap();

    // Set up a HTTPClient
    let mut client_builder = Client::builder().token(CONFIG.token.clone());

    if let Some(http_proxy) = CONFIG.twilight_http_proxy.clone() {
        client_builder = client_builder.proxy(http_proxy, true);
    }

    let client = Arc::new(client_builder.build());

    // Check total shards required
    let gateway = client.gateway().authed().exec().await?.model().await?;

    let shard_count = CONFIG.shards.unwrap_or(gateway.shards);

    // Set up a queue for the shards
    let queue: Arc<dyn Queue> = Arc::new(
        LargeBotQueue::new(gateway.session_start_limit.max_concurrency as usize, client).await,
    );

    // Create all shards
    let mut shards = Vec::with_capacity(shard_count as usize);

    info!("Creating {} shards", shard_count);

    for shard_id in 0..shard_count {
        let mut builder = Shard::builder(CONFIG.token.clone(), CONFIG.intents)
            .queue(queue.clone())
            .shard(shard_id, shard_count)?
            .gateway_url(Some(gateway.url.clone()))
            .event_types(CONFIG.cache.clone().into());

        if let Some(mut activity) = CONFIG.activity.clone() {
            // Replace {{shard}} with the actual ID
            activity.name = activity.name.replace("{{shard}}", &shard_id.to_string());
            // Will only error if activity are empty, so we can unwrap

            builder = builder.presence(
                UpdatePresencePayload::new(vec![activity], false, None, CONFIG.status).unwrap(),
            );
        }

        // To support multiple listeners on the same shard
        // we need to make a broadcast channel with the events
        let (broadcast_tx, _) = broadcast::channel(CONFIG.backpressure);

        let (shard, events) = builder.build();

        shard.start().await?;

        let cache = Arc::new(
            InMemoryCache::builder()
                .resource_types(CONFIG.cache.clone().into())
                .build(),
        );
        let guild_cache = cache::Guilds::new(cache.clone(), shard_id);
        let voice_cache = cache::Voice::new(cache, shard_id);

        let (ready_tx, ready_rx) = watch::channel(None);

        let shard_status = Arc::new(state::Shard {
            shard,
            events: broadcast_tx.clone(),
            ready: ready_rx,
            guilds: guild_cache,
            voice: voice_cache,
        });

        // Now pipe the events into the broadcast
        // and handle state updates for the guild cache
        // and set the ready event if received
        tokio::spawn(dispatch::events(
            events,
            shard_status.clone(),
            shard_id,
            broadcast_tx,
            ready_tx,
        ));

        // Track the shard latency in metrics
        tokio::spawn(dispatch::shard_latency(shard_status.clone()));

        shards.push(shard_status);

        debug!("Created shard {} of {} total", shard_id, shard_count);
    }

    let state = Arc::new(state::Inner {
        shards,
        shard_count,
    });

    if let Err(e) = server::run(CONFIG.port, state, metrics_handle).await {
        error!("{}", e);
    };

    Ok(())
}
