use futures_util::{SinkExt, StreamExt};
use log::{debug, info, trace, warn};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Error, Message},
};
use twilight_gateway::{shard::raw_message::Message as TwilightMessage, Event};
use twilight_model::gateway::{event::GatewayEventDeserializer, payload::outgoing::Identify};

use std::net::SocketAddr;

use crate::state::State;

const HELLO: &str = r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250}}"#;
const HEARTBEAT_ACK: &str = r#"{"t":null,"s":null,"op":11,"d":null}"#;
const INVALID_SESSION: &str = r#"{"t":null,"s":null,"op":9,"d":false}"#;

async fn forward_shard(
    mut shard_id_rx: UnboundedReceiver<u64>,
    stream_writer: UnboundedSender<Message>,
    mut shard_send_rx: UnboundedReceiver<TwilightMessage>,
    mut shutdown_reader_recv: UnboundedReceiver<()>,
    state: State,
) {
    let shard_id = shard_id_rx.recv().await.unwrap();
    // TODO: Shard could already be connected
    let (_, mut shard_status) = state.shards.remove(&shard_id).unwrap();

    debug!("Starting to send events to client");

    if !shard_status.first_time_used {
        // TODO: Should be safe but check
        let ready_payload = shard_status
            .guilds
            .get_ready_payload(shard_status.ready.clone().unwrap());

        if let Ok(serialized) = simd_json::to_string(&ready_payload) {
            debug!("Sending newly created READY");
            let _res = stream_writer.send(Message::Text(serialized));
        };

        for payload in shard_status.guilds.get_guild_payloads() {
            if let Ok(serialized) = simd_json::to_string(&payload) {
                trace!("Sending newly created GUILD_CREATE/GUILD_DELETE payload");
                let _res = stream_writer.send(Message::Text(serialized));
            };
        }
    }

    loop {
        tokio::select! {
            Some(event) = shard_status.events.next() => {
                if let Event::ShardPayload(body) = event {
                    let payload = unsafe { String::from_utf8_unchecked(body.bytes) };
                    // The event is always valid
                    let deserializer = GatewayEventDeserializer::from_json(&payload).unwrap();

                    if deserializer.op() == 0 && !deserializer.event_type_ref().contains(&"RESUMED") {
                        trace!("Sending payload to client: {:?}", payload);
                        let _res = stream_writer.send(Message::Text(payload));
                    }
                } else if let Event::Ready(mut ready) = event {
                    ready.guilds.clear();
                    shard_status.ready = Some(*ready);
                } else if let Event::GuildCreate(guild_create) = event {
                    shard_status.guilds.insert(guild_create.0);
                } else if let Event::GuildDelete(guild_delete) = event {
                    shard_status.guilds.remove(guild_delete.id);
                } else if let Event::GuildUpdate(guild_update) = event {
                    shard_status.guilds.update(guild_update.0);
                }
            },
            Some(command) = shard_send_rx.recv() => {
                // Has to be done here because else shard would be moved
                let _res = shard_status.shard.send(command).await;
            },
            _ = shutdown_reader_recv.recv() => {
                break;
            }
        };
    }

    debug!("Event stream ended or shutdown requested");

    shard_status.first_time_used = false;

    state.shards.insert(shard_id, shard_status);
}

async fn handle_client(stream: TcpStream, state: State) -> Result<(), Error> {
    let stream = accept_async(stream).await?;
    let (mut sink, mut stream) = stream.split();

    // Write all messages from a queue to the sink
    let (stream_writer, mut stream_receiver) = unbounded_channel();

    let sink_task = tokio::spawn(async move {
        while let Some(msg) = stream_receiver.recv().await {
            trace!("Sending {:?}", msg);
            sink.send(msg).await?;
        }

        Ok::<(), Error>(())
    });

    // Set up a task that will dump all the messages from the shard to the client
    let (shutdown_reader, shutdown_reader_recv) = unbounded_channel();
    let (shard_id_tx, shard_id_rx) = unbounded_channel();
    let (shard_send_tx, shard_send_rx) = unbounded_channel();

    tokio::spawn(forward_shard(
        shard_id_rx,
        stream_writer.clone(),
        shard_send_rx,
        shutdown_reader_recv,
        state.clone(),
    ));

    let _res = stream_writer.send(Message::Text(HELLO.to_string()));

    while let Some(Ok(msg)) = stream.next().await {
        let data = msg.into_data();
        let mut payload = unsafe { String::from_utf8_unchecked(data) };

        let deserializer = match GatewayEventDeserializer::from_json(&payload) {
            Some(deserializer) => deserializer,
            None => continue,
        };

        match deserializer.op() {
            1 => {
                trace!("Sending heartbeat ACK");
                let _res = stream_writer.send(Message::Text(HEARTBEAT_ACK.to_string()));
            }
            2 => {
                debug!("Client is identifying");

                let identify: Identify = match simd_json::from_str(&mut payload) {
                    Ok(identify) => identify,
                    Err(e) => {
                        warn!("Invalid identify payload: {}", e);
                        continue;
                    }
                };

                let (shard_id, shard_count) = if let Some(shard_data) = identify.d.shard {
                    (shard_data[0], shard_data[1])
                } else {
                    warn!("Client sent identify without shard ID, disconnecting");
                    break;
                };

                if shard_count != state.shard_count {
                    warn!("Shard count from client identify mismatched, disconnecting");
                    break;
                }

                trace!("Shard ID is {:?}", shard_id);

                let _res = shard_id_tx.send(shard_id);
            }
            6 => {
                debug!("Client is resuming: {:?}", payload);
                // TODO: Keep track of session IDs and choose one that we have active
                // This would be unnecessary if people forked their clients though
                // For now, send an invalid session so they use identify instead
                let _res = stream_writer.send(Message::text(INVALID_SESSION.to_string()));
            }
            _ => {
                trace!("Sending {:?} to Discord directly", payload);
                let _res = shard_send_tx.send(TwilightMessage::Text(payload));
            }
        }
    }

    sink_task.abort();
    let _res = shutdown_reader.send(());

    Ok(())
}

pub async fn run_server(port: u16, state: State) -> Result<(), Error> {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let listener = TcpListener::bind(addr).await?;

    info!("Listening on {}", addr);

    while let Ok((stream, remote_addr)) = listener.accept().await {
        info!("New connection from {}", remote_addr);

        tokio::spawn(handle_client(stream, state.clone()));
    }

    Ok(())
}
