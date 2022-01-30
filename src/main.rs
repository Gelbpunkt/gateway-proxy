#![feature(option_result_contains, once_cell)]
#![deny(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_ptr_alignment,
    clippy::struct_excessive_bools
)]
use libc::{c_int, sighandler_t, signal, SIGINT, SIGTERM};
use metrics_exporter_prometheus::PrometheusBuilder;
use mimalloc::MiMalloc;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, error, info};
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::Shard;
use twilight_gateway_queue::{LargeBotQueue, Queue};
use twilight_http::Client;
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;

use std::{collections::HashMap, error::Error, str::FromStr, sync::Arc};

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

async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let level_filter = LevelFilter::from_str(&CONFIG.log_level).unwrap_or(LevelFilter::INFO);
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(level_filter)
        .init();

    // Set up metrics collection
    let recorder = PrometheusBuilder::new().build_recorder();
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
    let shard_start = CONFIG.shard_start.unwrap_or(0);
    let shard_end = CONFIG.shard_end.unwrap_or(shard_count);
    let shard_end_inclusive = shard_end - 1;
    let mut shards = Vec::with_capacity((shard_end - shard_start) as usize);

    info!("Creating shards {shard_start} to {shard_end_inclusive} of {shard_count} total",);

    for shard_id in shard_start..shard_end {
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

        let ready = state::Ready::new();

        let shard_status = Arc::new(state::Shard {
            id: shard_id,
            shard,
            events: broadcast_tx.clone(),
            ready,
            guilds: guild_cache,
        });

        // Now pipe the events into the broadcast
        // and handle state updates for the guild cache
        // and set the ready event if received
        tokio::spawn(dispatch::events(
            events,
            shard_status.clone(),
            shard_id,
            broadcast_tx,
        ));

        // Track the shard latency in metrics
        tokio::spawn(dispatch::shard_latency(shard_status.clone()));

        shards.push(shard_status);

        debug!("Created shard {shard_id} of {shard_count} total");
    }

    let state = Arc::new(state::Inner {
        shards,
        shard_count,
        sessions: RwLock::new(HashMap::new()),
    });

    if let Err(e) = server::run(CONFIG.port, state, metrics_handle).await {
        error!("{}", e);
    };

    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    unsafe { set_os_handlers() };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run())
}
