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
use twilight_gateway::shard::raw_message::Message as TwilightMessage;
use twilight_model::gateway::event::GatewayEventDeserializer;

use std::net::SocketAddr;

use crate::{model::Identify, state::State};

const HELLO: &str = r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250}}"#;
const HEARTBEAT_ACK: &str = r#"{"t":null,"s":null,"op":11,"d":null}"#;
const INVALID_SESSION: &str = r#"{"t":null,"s":null,"op":9,"d":false}"#;

async fn forward_shard(
    mut shard_id_rx: UnboundedReceiver<u64>,
    stream_writer: UnboundedSender<Message>,
    mut shard_send_rx: UnboundedReceiver<TwilightMessage>,
    state: State,
) {
    let shard_id = shard_id_rx.recv().await.unwrap();
    let shard_status = state.shards[shard_id as usize].clone();

    let mut event_receiver = shard_status.events.subscribe();

    debug!("Starting to send events to client");

    if shard_status.ready.get().is_none() {
        shard_status.ready_set.notified().await;
    }

    // This is safe since ready is now set
    let ready_payload = shard_status
        .guilds
        .get_ready_payload(shard_status.ready.get().unwrap().clone());

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

    loop {
        tokio::select! {
            Ok(payload) = event_receiver.recv() => {
                let _res = stream_writer.send(Message::Text(payload));
            },
            Some(command) = shard_send_rx.recv() => {
                // Has to be done here because else shard would be moved
                let _res = shard_status.shard.send(command).await;
            },
        };
    }
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
    let (shard_id_tx, shard_id_rx) = unbounded_channel();
    let (shard_send_tx, shard_send_rx) = unbounded_channel();

    let shard_forward_task = tokio::spawn(forward_shard(
        shard_id_rx,
        stream_writer.clone(),
        shard_send_rx,
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
                        warn!("Invalid identify payload: {:?}", e);
                        continue;
                    }
                };

                let (shard_id, shard_count) = (identify.d.shard[0], identify.d.shard[1]);

                if shard_count != state.shard_count {
                    warn!("Shard count from client identify mismatched, disconnecting");
                    break;
                }

                if shard_id >= shard_count {
                    warn!("Shard ID from client is out of range, disconnecting");
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
    shard_forward_task.abort();

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
