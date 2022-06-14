use std::{error::Error, sync::Arc};

use futures_util::StreamExt;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};
use twilight_gateway::{Cluster, Intents};
use twilight_gateway_queue::NoOpQueue;
use twilight_http::Client;

async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(LevelFilter::DEBUG)
        .init();

    let token = std::env::var("TOKEN")?;

    let queue = Arc::new(NoOpQueue);

    let http = Arc::new(
        Client::builder()
            .token(token.clone())
            .proxy(String::from("localhost:8080"), true)
            .build(),
    );

    let (cluster, mut events) = Cluster::builder(token, Intents::all())
        .gateway_url(String::from("ws://localhost:7878"))
        .http_client(http)
        .ratelimit_payloads(false)
        .queue(queue)
        .build()
        .await?;

    cluster.up().await;

    while let Some((shard, event)) = events.next().await {
        let kind = event.kind();
        tracing::info!("Shard {shard}: {kind:?}");
        tracing::debug!("Event payload: {event:?}");
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run())
}
