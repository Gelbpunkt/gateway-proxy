#![feature(lazy_cell)]
#![deny(clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_ptr_alignment,
    clippy::struct_excessive_bools,
    clippy::option_if_let_else, // I disagree with this lint
)]
use metrics_exporter_prometheus::PrometheusBuilder;
use mimalloc::MiMalloc;
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::broadcast,
    task::JoinSet,
    time::timeout,
};
use tracing::{debug, error, info};
use tracing_subscriber::{
    filter::LevelFilter, layer::SubscriberExt, reload, util::SubscriberInitExt,
};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::{CloseFrame, ConfigBuilder, Shard, ShardId};
use twilight_gateway_queue::InMemoryQueue;
use twilight_http::Client;
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;

use std::{
    collections::HashMap,
    error::Error,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};

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

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::redundant_pub_crate
)]
async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let level_filter = LevelFilter::from_str(&CONFIG.log_level).unwrap_or(LevelFilter::INFO);
    let (reload_level_filter, reload_handle) = reload::Layer::new(level_filter);
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(reload_level_filter)
        .init();

    tokio::spawn(config::watch_config_changes(reload_handle));

    // Set up metrics collection
    let metrics_handle = PrometheusBuilder::new().install_recorder().unwrap();

    // Set up a HTTPClient
    let mut client_builder = Client::builder().token(CONFIG.token.clone());

    if let Some(http_proxy) = CONFIG.twilight_http_proxy.clone() {
        client_builder = client_builder.proxy(http_proxy, true);
    }

    let client = Arc::new(client_builder.build());

    // Check total shards required
    let gateway = client.gateway().authed().await?.model().await?;

    let session = gateway.session_start_limit;

    let shard_count = CONFIG.shards.unwrap_or(gateway.shards);

    // Set up a queue for the shards
    let queue = InMemoryQueue::new(
        session.max_concurrency,
        session.remaining,
        Duration::from_millis(session.reset_after),
        session.total,
    );

    // Create all shards
    let shard_start = CONFIG.shard_start.unwrap_or(0);
    let shard_end = CONFIG.shard_end.unwrap_or(shard_count);
    let shard_end_inclusive = shard_end - 1;
    let mut shards = Vec::with_capacity((shard_end - shard_start) as usize);

    info!("Creating shards {shard_start} to {shard_end_inclusive} of {shard_count} total",);

    let config = ConfigBuilder::new(CONFIG.token.clone(), CONFIG.intents)
        .queue(queue)
        .build();

    let mut dispatch_tasks = JoinSet::new();

    for shard_id in shard_start..shard_end {
        let mut builder = ConfigBuilder::from(config.clone());

        if let Some(mut activity) = CONFIG.activity.clone() {
            // Replace {{shard}} with the actual ID
            activity.name = activity.name.replace("{{shard}}", &shard_id.to_string());
            // Will only error if activities are empty, so we can unwrap
            builder = builder.presence(
                UpdatePresencePayload::new(vec![activity], false, None, CONFIG.status).unwrap(),
            );
        }

        let shard = Shard::with_config(ShardId::new(shard_id, shard_count), builder.build());

        // To support multiple listeners on the same shard
        // we need to make a broadcast channel with the events
        let (broadcast_tx, _) = broadcast::channel(CONFIG.backpressure);

        let cache = Arc::new(
            InMemoryCache::builder()
                .resource_types(CONFIG.cache.clone().into())
                .message_cache_size(0)
                .build(),
        );
        let guild_cache = cache::Guilds::new(cache.clone());

        let ready = state::Ready::new();

        let shard_status = Arc::new(state::Shard {
            id: shard_id,
            sender: shard.sender(),
            events: broadcast_tx.clone(),
            ready,
            guilds: guild_cache,
        });

        // Now pipe the events into the broadcast
        // and handle state updates for the guild cache
        // and set the ready event if received
        dispatch_tasks.spawn(dispatch::events(
            shard,
            shard_status.clone(),
            shard_id,
            broadcast_tx,
        ));

        shards.push(shard_status);

        debug!("Created shard {shard_id} of {shard_count} total");
    }

    let state = Arc::new(state::Inner {
        shards,
        shard_count,
        sessions: RwLock::new(HashMap::new()),
    });

    let state_clone = state.clone();
    tokio::spawn(async move {
        if let Err(e) = server::run(CONFIG.port, state_clone, metrics_handle).await {
            error!("{}", e);
        }
    });

    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    let mut sigterm = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigint.recv() => info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
    }

    // Set the flag so that event handlers will be able to tell that a GatewayClose is an expected shutdown
    SHUTDOWN.store(true, Ordering::Relaxed);

    // Initiate the shutdown for all shards
    for shard in &state.shards {
        let _ = shard.sender.close(CloseFrame::NORMAL);
    }

    let mut graceful = 0;
    let mut ungraceful = dispatch_tasks.len();

    // Wait for all shards to shut down, but if we for some reason fail to do so, exit anyways
    info!("waiting for {ungraceful} active shard dispatching tasks to shut down");

    loop {
        match timeout(Duration::from_secs(10), dispatch_tasks.join_next()).await {
            Ok(Some(_)) => {
                debug!("shard dispatching task shut down");
                graceful += 1;
                ungraceful -= 1;
            } // Shard task shut down
            Ok(None) => break, // Set is empty, all tasks were graceful
            Err(_) => {
                error!("no shard shut down within 10 seconds, force quitting");
                break;
            } // No shard shut down in 10s, remaining ones are ungraceful
        }
    }

    info!("{graceful} shards shut down gracefully, {ungraceful} not gracefully");

    Ok(())
}

fn main() {
    if let Err(e) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run())
    {
        eprintln!("Fatal error: {e}");
    }
}
