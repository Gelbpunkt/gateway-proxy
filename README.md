# gateway-proxy

> This is a very hacky project, so it might stop working if Discord changes their API core. This is unlikely, but keep that in mind while using the proxy.

This is a proxy for Discord gateway connections - clients can connect to this proxy instead of the Discord Gateway and interact with it just like they would with the Discord Gateway.

The proxy connects to Discord instead of the client - allowing for zero-downtime client restarts while the proxy keeps its connections to the gateway open. The proxy won't invalidate your sessions or disconnect you (exceptions below).

## How?

It connects all shards to Discord upfront and mimics to be the actual API gateway.

When a client sends an `IDENTIFY` payload, it takes the shard ID specified and relays all events for that shard to the client.

It also sends you self-crafted, but valid `READY` and `GUILD_CREATE`/`GUILD_DELETE` payloads at startup to keep your guild state up to date, just like Discord does, even though it doesn't reconnect when you do internally.

## Configuration

Create a file `config.json` and fill in these fields:

```json
{
  "log_level": "info",
  "token": "",
  "intents": 32511
}
```

## Connecting

Connecting is fairly simple, just hardcode the gateway URL in your client to `ws://localhost:7878`. Make sure not to ratelimit your connections on your end.

## Caveats

The proxy uses zlib-stream for its connection to Discord's gateway, but only supports plain JSON for communication with the client. To ensure this behaviour, all websocket messages are of `TEXT` type and not `BINARY`.

`RESUME`s will always result in a session invalidation because the proxy doesn't really track session IDs. Just reidentify, it's free.

## Known Issues / TODOs

- Sequence numbers are very wrong
- Some assumptions about state are being made that shouldn't be to ensure safety
