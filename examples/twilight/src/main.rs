use std::{error::Error, sync::Arc};

use futures_util::StreamExt;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt};
use twilight_gateway::{
    stream::{create_recommended, ShardEventStream},
    Config, ConfigBuilder, Intents,
};
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

    let config = Config::builder(token.clone(), Intents::all())
        .proxy_url(String::from("ws://localhost:7878"))
        .ratelimit_messages(false)
        .queue(queue.clone())
        .build();
    let config_callback = |_, builder: ConfigBuilder| builder.build();

    let mut shards = create_recommended(&http, config, config_callback)
        .await?
        .collect::<Vec<_>>();
    let mut stream = ShardEventStream::new(shards.iter_mut());

    while let Some((shard, Ok(event))) = stream.next().await {
        let id = shard.id();
        let kind = event.kind();
        tracing::info!("Shard {id}: {kind:?}");
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
