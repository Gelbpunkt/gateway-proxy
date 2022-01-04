use flate2::{Compress, Compression, FlushCompress};
use futures_util::{Future, SinkExt, StreamExt};
use hyper::{
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Error, Message},
    WebSocketStream,
};
use tracing::{debug, error, info, trace, warn};
use twilight_gateway::shard::raw_message::Message as TwilightMessage;
use twilight_model::id::{marker::GuildMarker, Id};

use std::{convert::Infallible, net::SocketAddr, pin::Pin, sync::Arc};

use crate::{
    config::CONFIG,
    deserializer::{GatewayEvent, SequenceInfo},
    model::{Identify, VoiceStateUpdate},
    state::State,
    upgrade,
};

const HELLO: &str = r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250}}"#;
const HEARTBEAT_ACK: &str = r#"{"t":null,"s":null,"op":11,"d":null}"#;
const INVALID_SESSION: &str = r#"{"t":null,"s":null,"op":9,"d":false}"#;

enum SendMessage {
    Message(TwilightMessage),
    Voice(Id<GuildMarker>),
}

async fn forward_shard(
    mut shard_id_rx: UnboundedReceiver<u64>,
    stream_writer: UnboundedSender<Message>,
    mut shard_send_rx: UnboundedReceiver<SendMessage>,
    state: State,
) {
    // Wait for the client's IDENTIFY to finish and acquire the shard ID
    let shard_id = shard_id_rx.recv().await.unwrap();
    // Get a handle to the shard
    let shard_status = state.shards[shard_id as usize].clone();

    let mut ready_rx = shard_status.ready.clone();

    // Fake sequence number for the client. We update packets to overwrite it
    let mut seq: usize = 0;

    // Subscribe to events for this shard
    let mut event_receiver = shard_status.events.subscribe();

    debug!("[Shard {}] Starting to send events to client", shard_id);

    // If there is no READY received for the shard yet, wait for it
    if ready_rx.borrow_and_update().is_none() {
        let _ = ready_rx.changed().await;
    }

    {
        // Get a fake ready payload to send to the client
        let ready_payload = shard_status
            .guilds
            .get_ready_payload(ready_rx.borrow().clone().unwrap(), &mut seq);

        if let Ok(serialized) = simd_json::to_string(&ready_payload) {
            debug!("[Shard {}] Sending newly created READY", shard_id);
            let _res = stream_writer.send(Message::Text(serialized));
        };

        // Send GUILD_CREATE/GUILD_DELETEs based on guild availability
        for payload in shard_status.guilds.get_guild_payloads(&mut seq) {
            if let Ok(serialized) = simd_json::to_string(&payload) {
                trace!(
                    "[Shard {}] Sending newly created GUILD_CREATE/GUILD_DELETE payload",
                    shard_id
                );
                let _res = stream_writer.send(Message::Text(serialized));
            };
        }
    }

    loop {
        tokio::select! {
            Ok(event) = event_receiver.recv() => {
                // A payload has arrived to be sent to the client
                let (mut payload, sequence) = event;

                // Overwrite the sequence number
                if let Some(SequenceInfo(_, sequence_range)) = sequence {
                    seq += 1;
                    payload.replace_range(sequence_range, &seq.to_string());
                }

                let _res = stream_writer.send(Message::Text(payload));
            },
            Some(command) = shard_send_rx.recv() => {
                // Has to be done here because else shard would be moved
                match command {
                    SendMessage::Message(msg) => {
                        let _res = shard_status.shard.send(msg).await;
                    },
                    SendMessage::Voice(guild_id) => {
                        let payloads = shard_status.voice.get_payloads(guild_id, &mut seq);
                        for payload in payloads {
                            if let Ok(serialized) = simd_json::to_string(&payload) {
                                debug!(
                                    "[Shard {}] Sending newly created VOICE_STATE_UPDATE/VOICE_SERVER_UPDATE payload",
                                    shard_id
                                );
                                let _res = stream_writer.send(Message::Text(serialized));
                            };
                        }
                    }
                }
            },
            _ = ready_rx.changed() => {
                if ready_rx.borrow().is_none() {
                    debug!("[Shard {}] Temporary disconnect, stopped forwarding events", shard_id);
                    let _ = ready_rx.changed().await;
                    debug!("[Shard {}] Reconnected, starting to forward events again", shard_id);
                }
            }
        };
    }
}

pub async fn handle_client<S: 'static + AsyncRead + AsyncWrite + Unpin + Send>(
    addr: SocketAddr,
    stream: S,
    state: State,
    mut use_zlib: bool,
) -> Result<(), Error> {
    // We use a oneshot channel to tell the forwarding task whether the IDENTIFY
    // contained a compression request
    let (compress_tx, compress_rx) = oneshot::channel();
    let mut compress_tx = Some(compress_tx);

    // Just set it to 0 because at least that'll always exist
    // We will always set it later
    let mut client_shard_id = 0;
    let mut compress = Compress::new(Compression::fast(), true);

    let stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

    let (mut sink, mut stream) = stream.split();

    // Because we wait for IDENTIFY later, HELLO needs to be sent now
    // and optionally compressed!
    if use_zlib {
        let mut compressed = Vec::with_capacity(HELLO.len());
        compress
            .compress_vec(HELLO.as_bytes(), &mut compressed, FlushCompress::Sync)
            .unwrap();

        sink.send(Message::Binary(compressed)).await?;
    } else {
        sink.send(Message::Text(HELLO.to_string())).await?;
    }

    // Write all messages from a queue to the sink
    let (stream_writer, mut stream_receiver) = unbounded_channel::<Message>();

    let sink_task = tokio::spawn(async move {
        if compress_rx.await.contains(&Some(true)) {
            use_zlib = true;
        }

        while let Some(msg) = stream_receiver.recv().await {
            trace!("[{}] Sending {:?}", addr, msg);

            if use_zlib {
                let mut compressed = Vec::with_capacity(msg.len());
                compress
                    .compress_vec(&msg.into_data(), &mut compressed, FlushCompress::Sync)
                    .unwrap();

                sink.send(Message::Binary(compressed)).await?;
            } else {
                sink.send(msg).await?;
            }
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

    while let Some(Ok(msg)) = stream.next().await {
        let data = msg.into_data();
        let mut payload = unsafe { String::from_utf8_unchecked(data) };

        let deserializer = match GatewayEvent::from_json(&payload) {
            Some(deserializer) => deserializer,
            None => continue,
        };

        match deserializer.op() {
            1 => {
                trace!("[{}] Sending heartbeat ACK", addr);
                let _res = stream_writer.send(Message::Text(HEARTBEAT_ACK.to_string()));
            }
            2 => {
                debug!("[{}] Client is identifying", addr);

                let identify: Identify = match simd_json::from_str(&mut payload) {
                    Ok(identify) => identify,
                    Err(e) => {
                        warn!("[{}] Invalid identify payload: {:?}", addr, e);
                        continue;
                    }
                };

                let (shard_id, shard_count) = (identify.d.shard[0], identify.d.shard[1]);

                if shard_count != state.shard_count {
                    warn!(
                        "[{}] Shard count from client identify mismatched, disconnecting",
                        addr
                    );
                    break;
                }

                if shard_id >= shard_count {
                    warn!(
                        "[{}] Shard ID from client is out of range, disconnecting",
                        addr
                    );
                    break;
                }

                if identify.d.token != CONFIG.token {
                    warn!("[{}] Token from client mismatched, disconnecting", addr);
                    break;
                }

                trace!("[{}] Shard ID is {:?}", addr, shard_id);

                if let Some(sender) = compress_tx.take() {
                    let _res = sender.send(identify.d.compress);
                }

                client_shard_id = shard_id as usize;
                let _res = shard_id_tx.send(shard_id);
            }
            6 => {
                debug!("[{}] Client is resuming", addr);
                // TODO: Keep track of session IDs and choose one that we have active
                // This would be unnecessary if people forked their clients though
                // For now, send an invalid session so they use identify instead
                let _res = stream_writer.send(Message::text(INVALID_SESSION.to_string()));
            }
            4 => {
                debug!("[{}] Client is connecting to voice", addr);

                let voice_state: VoiceStateUpdate = match simd_json::from_str(&mut payload.clone())
                {
                    Ok(voice_state) => voice_state,
                    Err(e) => {
                        warn!("[{}] Invalid voice state update payload: {:?}", addr, e);
                        continue;
                    }
                };

                let shard = state.shards[client_shard_id].clone();

                // If the bot isn't in this channel already, forward it
                let is_in_vc = if let Some(channel_id) = voice_state.d.channel_id {
                    shard
                        .voice
                        .is_in_channel(voice_state.d.guild_id, channel_id)
                } else {
                    // It is disconnecting
                    shard.voice.disconnect(voice_state.d.guild_id);

                    false
                };

                if is_in_vc {
                    let _res = shard_send_tx.send(SendMessage::Voice(voice_state.d.guild_id));
                } else {
                    let _res =
                        shard_send_tx.send(SendMessage::Message(TwilightMessage::Text(payload)));
                }
            }
            _ => {
                trace!("[{}] Sending {:?} to Discord directly", addr, payload);
                let _res = shard_send_tx.send(SendMessage::Message(TwilightMessage::Text(payload)));
            }
        }
    }

    debug!("[{}] Client disconnected", addr);

    sink_task.abort();
    shard_forward_task.abort();

    Ok(())
}

fn handle_metrics(
    handle: Arc<PrometheusHandle>,
) -> Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>> {
    Box::pin(async move {
        Ok(Response::builder()
            .body(Body::from(handle.render()))
            .unwrap())
    })
}

pub async fn run(
    port: u16,
    state: State,
    metrics_handle: Arc<PrometheusHandle>,
) -> Result<(), Error> {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    let service = make_service_fn(move |addr: &AddrStream| {
        let state = state.clone();
        let metrics_handle = metrics_handle.clone();
        let addr = addr.remote_addr();

        trace!("[{:?}] New connection", addr);

        async move {
            Ok::<_, Infallible>(service_fn(move |incoming: Request<Body>| {
                if incoming.uri().path() == "/metrics" {
                    // Reply with metrics on /metrics
                    handle_metrics(metrics_handle.clone())
                } else {
                    // On anything else just provide the websocket server
                    Box::pin(upgrade::server(addr, incoming, state.clone()))
                }
            }))
        }
    });

    let server = Server::bind(&addr).serve(service);

    info!("Listening on {}", addr);

    if let Err(why) = server.await {
        error!("Fatal server error: {}", why);
    }

    Ok(())
}
