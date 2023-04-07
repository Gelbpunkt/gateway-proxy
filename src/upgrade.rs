use base64::{engine::general_purpose::STANDARD, Engine};
use hyper::{
    header::{
        HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_VERSION,
        UPGRADE,
    },
    http::StatusCode,
    upgrade, Body, Request, Response,
};
use ring::digest;
use tracing::error;

use std::{convert::Infallible, net::SocketAddr};

use crate::{server::handle_client, state::State};

/// Websocket GUID constant as specified in RFC6455:
/// <https://datatracker.ietf.org/doc/html/rfc6455#section-1.3>
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Accept a websocket upgrade request and start processing the client's
/// events afterwards.
///
/// This method is one of two parts in the communication between server
/// and client where zlib-stream compression may be requested.
pub async fn server(
    addr: SocketAddr,
    mut request: Request<Body>,
    state: State,
) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();
    let query = uri.query();

    // Track whether the client requested zlib encoding in the query
    // string parameters
    let use_zlib = query.map_or(false, |q| q.contains("compress=zlib-stream"));

    let mut response = Response::new(Body::empty());

    if request.headers().get(UPGRADE).and_then(|v| v.to_str().ok()) != Some("websocket") {
        *response.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(response);
    }

    if let Some(websocket_key) = request.headers().get(SEC_WEBSOCKET_KEY) {
        let mut ctx = digest::Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
        ctx.update(websocket_key.as_bytes());
        ctx.update(GUID.as_bytes());
        let accept_key = STANDARD.encode(ctx.finish().as_ref());

        // Spawn a task that waits for the upgrade to finish to
        // get access to the underlying connection
        tokio::spawn(async move {
            match upgrade::on(&mut request).await {
                Ok(upgraded) => {
                    let _res = handle_client(addr, upgraded, state, use_zlib).await;
                }
                Err(e) => error!("[{}] Websocket upgrade error: {}", addr, e),
            }
        });

        *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
        response
            .headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("Upgrade"));
        response
            .headers_mut()
            .insert(UPGRADE, HeaderValue::from_static("websocket"));
        response.headers_mut().insert(
            SEC_WEBSOCKET_ACCEPT,
            HeaderValue::from_str(&accept_key).unwrap(),
        );
        response
            .headers_mut()
            .insert(SEC_WEBSOCKET_VERSION, HeaderValue::from_static("13"));
    } else {
        *response.status_mut() = StatusCode::BAD_REQUEST;
    }

    Ok(response)
}
