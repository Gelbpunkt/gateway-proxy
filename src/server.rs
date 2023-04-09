use flate2::{Compress, Compression, FlushCompress, Status};
use futures_util::{Sink, SinkExt, StreamExt};
use hyper::{
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server, StatusCode,
};
use itoa::Buffer;
use metrics_exporter_prometheus::PrometheusHandle;
#[cfg(not(feature = "simd-json"))]
use serde_json::{to_string, Value as OwnedValue};
#[cfg(feature = "simd-json")]
use simd_json::{to_string, OwnedValue};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{
        broadcast::error::RecvError,
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};
use tokio_tungstenite::{
    tungstenite::{protocol::Role, Error, Message},
    WebSocketStream,
};
use tracing::{debug, error, info, trace, warn};

use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use crate::{
    cache::{Event, Payload},
    config::CONFIG,
    deserializer::{GatewayEvent, SequenceInfo},
    model::{Identify, Resume, VoiceStateUpdate},
    state::{Session, Shard, State},
    upgrade,
};

const HELLO: &str = r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250}}"#;
const HEARTBEAT_ACK: &str = r#"{"t":null,"s":null,"op":11,"d":null}"#;
const INVALID_SESSION: &str = r#"{"t":null,"s":null,"op":9,"d":false}"#;
const RESUMED: &str = r#"{"t":"RESUMED","s":null,"op":0,"d":{}}"#;

const TRAILER: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

fn compress_full(compressor: &mut Compress, output: &mut Vec<u8>, input: &[u8]) {
    let before_in = compressor.total_in() as usize;
    while (compressor.total_in() as usize) - before_in < input.len() {
        let offset = (compressor.total_in() as usize) - before_in;
        match compressor
            .compress_vec(&input[offset..], output, FlushCompress::None)
            .unwrap()
        {
            Status::Ok => continue,
            Status::BufError => output.reserve(4096),
            Status::StreamEnd => break,
        }
    }

    while !output.ends_with(&TRAILER) {
        output.reserve(5);
        match compressor
            .compress_vec(&[], output, FlushCompress::Sync)
            .unwrap()
        {
            Status::Ok | Status::BufError => continue,
            Status::StreamEnd => break,
        }
    }
}

async fn sink_from_queue<S>(
    addr: SocketAddr,
    mut use_zlib: bool,
    compress_rx: oneshot::Receiver<Option<bool>>,
    mut message_stream: UnboundedReceiver<Message>,
    mut sink: S,
) -> Result<(), Error>
where
    S: Sink<Message, Error = Error> + Unpin + Send,
{
    // Initialize a zlib encoder with similar settings to Discord's
    let mut compress = Compress::new(Compression::fast(), true);
    let mut compression_buffer = Vec::with_capacity(32 * 1024);

    // At first, we will have to send a HELLO
    if use_zlib {
        compress_full(&mut compress, &mut compression_buffer, HELLO.as_bytes());

        sink.send(Message::Binary(compression_buffer.clone()))
            .await?;
    } else {
        sink.send(Message::Text(HELLO.to_string())).await?;
    }

    if compress_rx.await == Ok(Some(true)) {
        use_zlib = true;
    }

    while let Some(msg) = message_stream.recv().await {
        trace!("[{addr}] Sending {msg:?}");

        if use_zlib {
            compression_buffer.clear();
            compress_full(&mut compress, &mut compression_buffer, &msg.into_data());

            sink.send(Message::Binary(compression_buffer.clone()))
                .await?;
        } else {
            sink.send(msg).await?;
        }
    }

    Ok(())
}

async fn forward_shard(
    session_id: String,
    shard_status: Arc<Shard>,
    stream_writer: UnboundedSender<Message>,
    mut client_responses: UnboundedReceiver<Payload>,
    send_guilds: bool,
    mut seq: usize,
) {
    // Subscribe to events for this shard
    let mut event_receiver = shard_status.events.subscribe();
    let shard_id = shard_status.id;

    debug!("[Shard {shard_id}] Starting to send events to client",);

    // Wait until we have a valid READY payload for this shard
    let ready_payload = shard_status.ready.wait_until_ready().await;

    if send_guilds {
        // Get a fake ready payload to send to the client
        let mut ready_payload = shard_status
            .cache
            .get_ready_payload(ready_payload, &mut seq);

        // Overwrite the session ID in the READY
        if let Event::Ready(payload) = &mut ready_payload.d {
            payload.insert(String::from("session_id"), OwnedValue::String(session_id));
        }

        if let Ok(serialized) = to_string(&ready_payload) {
            debug!("[Shard {shard_id}] Sending newly created READY");
            let _res = stream_writer.send(Message::Text(serialized));
        };

        // Send GUILD_CREATE/GUILD_DELETEs based on guild availability
        for payload in shard_status.cache.get_guild_payloads(&mut seq) {
            if let Ok(serialized) = to_string(&payload) {
                trace!(
                    "[Shard {shard_id}] Sending newly created GUILD_CREATE/GUILD_DELETE payload",
                );
                let _res = stream_writer.send(Message::Text(serialized));
            };
        }
    } else {
        let _res = stream_writer.send(Message::Text(RESUMED.to_string()));
    }

    // For formatting the sequence number as a string, reuse a buffer
    let mut buffer = Buffer::new();

    loop {
        tokio::select! {
            res = event_receiver.recv() => {
                if let Ok((mut payload, sequence)) = res {
                    // Overwrite the sequence number
                    if let Some(SequenceInfo(_, sequence_range)) = sequence {
                        seq += 1;
                        payload.replace_range(sequence_range, buffer.format(seq));
                    }

                    let _res = stream_writer.send(Message::Text(payload));
                } else if let Err(RecvError::Lagged(amt)) = res {
                    warn!("[Shard {shard_id}] Client is {amt} events behind!",);
                }
            },
            maybe_payload = client_responses.recv() => {
                if let Some(mut payload) = maybe_payload {
                    seq += 1;
                    payload.s = seq;

                    match to_string(&payload) {
                        Ok(payload) => {
                            let _res = stream_writer.send(Message::Text(payload));
                        },
                        Err(e) => {
                            warn!("Failed to serialize payload for client due to {e:?}");
                        }
                    }
                } else {
                    warn!("Client response stream has been closed");
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
pub async fn handle_client<S: 'static + AsyncRead + AsyncWrite + Unpin + Send>(
    addr: SocketAddr,
    stream: S,
    state: State,
    use_zlib: bool,
) -> Result<(), Error> {
    // We use a oneshot channel to tell the forwarding task whether the IDENTIFY
    // contained a compression request
    let (compress_tx, compress_rx) = oneshot::channel();
    let mut compress_tx = Some(compress_tx);

    let stream = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;

    let (sink, mut stream) = stream.split();

    // Write all messages from a queue to the sink
    let (stream_writer, stream_receiver) = unbounded_channel::<Message>();

    let sink_task = tokio::spawn(sink_from_queue(
        addr,
        use_zlib,
        compress_rx,
        stream_receiver,
        sink,
    ));

    let mut shard_forward_task = None;
    let mut client_response_sender = None;
    let mut shard: Option<Arc<Shard>> = None;

    while let Some(Ok(msg)) = stream.next().await {
        let data = msg.into_data();
        #[cfg(feature = "simd-json")]
        let mut payload = unsafe { String::from_utf8_unchecked(data) };
        #[cfg(not(feature = "simd-json"))]
        let payload = unsafe { String::from_utf8_unchecked(data) };

        let Some(deserializer) = GatewayEvent::from_json(&payload) else { continue };

        match deserializer.op() {
            1 => {
                trace!("[{addr}] Sending heartbeat ACK");
                let _res = stream_writer.send(Message::Text(HEARTBEAT_ACK.to_string()));
            }
            2 => {
                debug!("[{addr}] Client is identifying");

                #[cfg(feature = "simd-json")]
                let maybe_identify = unsafe { simd_json::from_str(&mut payload) };
                #[cfg(not(feature = "simd-json"))]
                let maybe_identify = serde_json::from_str(&payload);

                let identify: Identify = match maybe_identify {
                    Ok(identify) => identify,
                    Err(e) => {
                        warn!("[{addr}] Invalid identify payload: {e:?}");
                        continue;
                    }
                };

                let (shard_id, shard_count) = (identify.d.shard[0], identify.d.shard[1]);

                if shard_count != state.shard_count {
                    warn!("[{addr}] Shard count from client identify mismatched, disconnecting",);
                    break;
                }

                if shard_id >= shard_count {
                    warn!("[{addr}] Shard ID from client is out of range, disconnecting",);
                    break;
                }

                // Discord tokens may be prefixed by 'Bot ' in IDENTIFY
                if identify.d.token.split_whitespace().last() != Some(&CONFIG.token) {
                    warn!("[{addr}] Token from client mismatched, disconnecting");
                    break;
                }

                trace!("[{addr}] Shard ID is {shard_id}");

                // Create a new session for this client
                let session = Session {
                    shard_id,
                    compress: identify.d.compress,
                };
                let session_id = state.create_session(session);

                // The client is connected to this shard, so prepare for sending commands to it
                let local_shard = state.shards[shard_id as usize].clone();
                shard = Some(local_shard.clone());

                if let Some(sender) = compress_tx.take() {
                    let (tx, rx) = unbounded_channel();

                    shard_forward_task = Some(tokio::spawn(forward_shard(
                        session_id,
                        local_shard,
                        stream_writer.clone(),
                        rx,
                        true,
                        0,
                    )));

                    client_response_sender = Some(tx);

                    let _res = sender.send(identify.d.compress);
                }
            }
            4 => {
                debug!("[{addr}] Client is sending a voice state update");

                let Some(shard) = &shard else {
                    warn!("Client sent voice state update before identifying or resuming");
                    continue;
                };

                #[cfg(feature = "simd-json")]
                let maybe_voice_state_update = unsafe { simd_json::from_str(&mut payload) };
                #[cfg(not(feature = "simd-json"))]
                let maybe_voice_state_update = serde_json::from_str(&payload);

                let voice_state_update: VoiceStateUpdate = match maybe_voice_state_update {
                    Ok(voice_state_update) => voice_state_update,
                    Err(e) => {
                        warn!("[{addr}] Invalid voice state update payload: {e:?}");
                        continue;
                    }
                };

                // Check if this can be retrieved from cache
                if let Some((voice_state_update, voice_server_update)) =
                    voice_state_update.d.channel_id.and_then(|channel_id| {
                        shard.cache.get_voice_state_update_response(
                            voice_state_update.d.guild_id,
                            channel_id,
                        )
                    })
                {
                    if let Some(sender) = &client_response_sender {
                        debug!("Sending cached voice state update and voice server update");

                        let _ = sender.send(voice_state_update);
                        let _ = sender.send(voice_server_update);
                    } else {
                        error!("Client response sender has not been initialized, this should be impossible");
                    }
                } else {
                    trace!("[{addr}] Sending {payload:?} to Discord directly");
                    let _res = shard.sender.send(payload);
                }
            }
            6 => {
                debug!("[{addr}] Client is resuming");

                #[cfg(feature = "simd-json")]
                let maybe_resume = unsafe { simd_json::from_str(&mut payload) };
                #[cfg(not(feature = "simd-json"))]
                let maybe_resume = serde_json::from_str(&payload);

                let resume: Resume = match maybe_resume {
                    Ok(resume) => resume,
                    Err(e) => {
                        warn!("[{addr}] Invalid resume payload: {e:?}");
                        continue;
                    }
                };

                // Discord tokens may be prefixed by 'Bot ' in RESUME
                if resume.d.token.split_whitespace().last() != Some(&CONFIG.token) {
                    warn!("[{addr}] Token from client mismatched, disconnecting");
                    break;
                }

                // Find the shard that has the matching session ID
                if let Some(session) = state.get_session(&resume.d.session_id) {
                    let session_id = resume.d.session_id;
                    debug!("[{addr}] Successfully resuming session {session_id}",);

                    let local_shard = state.shards[session.shard_id as usize].clone();
                    shard = Some(local_shard.clone());

                    if let Some(sender) = compress_tx.take() {
                        let (tx, rx) = unbounded_channel();

                        shard_forward_task = Some(tokio::spawn(forward_shard(
                            session_id,
                            local_shard,
                            stream_writer.clone(),
                            rx,
                            false,
                            resume.d.seq,
                        )));

                        client_response_sender = Some(tx);

                        let _res = sender.send(session.compress);
                    } else {
                        let _res = stream_writer.send(Message::text(INVALID_SESSION.to_string()));
                    }
                } else {
                    let _res = stream_writer.send(Message::text(INVALID_SESSION.to_string()));
                }
            }
            _ => {
                if let Some(shard) = &shard {
                    trace!("[{addr}] Sending {payload:?} to Discord directly");
                    let _res = shard.sender.send(payload);
                } else {
                    warn!("[{addr}] Client attempted to send payload before IDENTIFY",);
                }
            }
        }
    }

    debug!("[{addr}] Client disconnected");

    sink_task.abort();

    if let Some(shard_forward_task) = shard_forward_task {
        shard_forward_task.abort();
    }

    Ok(())
}

async fn handler(
    addr: SocketAddr,
    request: Request<Body>,
    state: State,
    metrics: Arc<PrometheusHandle>,
) -> Result<Response<Body>, Infallible> {
    let response = match (request.method(), request.uri().path()) {
        (&Method::GET, "/metrics") => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(metrics.render()))
            .unwrap(),
        (&Method::GET, "/shard-count") => {
            let mut buffer = itoa::Buffer::new();
            let shard_count_str = buffer.format(state.shard_count);

            Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(shard_count_str.to_owned()))
                .unwrap()
        }
        // Usually one would return a 404 here, but we will just provide the websocket
        // upgrade for backwards compatibility.
        _ => upgrade::server(addr, request, state).await,
    };

    Ok(response)
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

        trace!("[{addr:?}] New connection");

        async move {
            Ok::<_, Infallible>(service_fn(move |incoming: Request<Body>| {
                handler(addr, incoming, state.clone(), metrics_handle.clone())
            }))
        }
    });

    let server = Server::bind(&addr).serve(service);

    info!("Listening on {addr}");

    if let Err(why) = server.await {
        error!("Fatal server error: {why}");
    }

    Ok(())
}
