use base64::encode;
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

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub async fn server_upgrade(
    addr: SocketAddr,
    mut request: Request<Body>,
    state: State,
) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();
    let query = uri.query();

    // Track whether the client requested zlib encoding
    let use_zlib = query.map_or(false, |q| q.contains(&"compress=zlib-stream"));

    let mut response = Response::new(Body::empty());

    if !request.headers().contains_key(UPGRADE)
        || !request.headers().contains_key(SEC_WEBSOCKET_KEY)
    {
        *response.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(response);
    }

    let upgrade = request.headers().get(UPGRADE).unwrap();

    if upgrade.to_str().unwrap() != "websocket" {
        *response.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(response);
    }

    let key = request.headers().get(SEC_WEBSOCKET_KEY).unwrap();

    let mut ctx = digest::Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    ctx.update(key.as_bytes());
    ctx.update(GUID.as_bytes());
    let accept_key = encode(ctx.finish().as_ref());

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
    Ok(response)
}
