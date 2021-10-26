# gateway-proxy

> This is a very hacky project, so it might stop working if Discord changes their API core. This is unlikely, but keep that in mind while using the proxy.

This is a proxy for Discord gateway connections - clients can connect to this proxy instead of the Discord Gateway and interact with it just like they would with the Discord Gateway.

The proxy connects to Discord instead of the client - allowing for zero-downtime client restarts while the proxy keeps its connections to the gateway open. The proxy won't invalidate your sessions or disconnect you (exceptions below).

## How?

It connects all shards to Discord upfront and mimics to be the actual API gateway.

When a client sends an `IDENTIFY` payload, it takes the shard ID specified and relays all events for that shard to the client.

It also sends you self-crafted, but valid `READY` and `GUILD_CREATE`/`GUILD_DELETE` payloads at startup to keep your guild state up to date, just like Discord does, even though it doesn't reconnect when you do internally.

Because the `IDENTIFY` is not actually controlled by the client side, activity data must be specified in the config file and will have no effect when sent in the client's `IDENTIFY` payload.

## Configuration

Create a file `config.json` and fill in these fields:

```json
{
  "log_level": "info",
  "token": "",
  "intents": 32511,
  "port": 7878,
  "shards": null,
  "activity": {
    "type": 0,
    "name": "with kubernetes"
  },
  "status": "idle",
  "backpressure": 100
}
```

## Connecting

Connecting is fairly simple, just hardcode the gateway URL in your client to `ws://localhost:7878`. Make sure not to ratelimit your connections on your end.

**Important:** The proxy detects `zlib-stream` query parameters and `compress` fields in your `IDENTIFY` payloads and will encode packets if they are enabled, just like Discord. This codes with CPU overhead and is likely not desired in localhost networking. Make sure to disable this if so.

## Caveats

`RESUME`s will always result in a session invalidation because the proxy doesn't really track session IDs. Just reidentify, it's free.

## Known Issues / TODOs

- Sequence numbers are very wrong
- Some clients think they are lagging because the proxy does not request heartbeats by itself
- Send `GUILD_MEMBER_UPDATE`s for the bot itself in each guild
- `GUILD_CREATE` has more state that needs to be tracked. Probably voice states, threads, stickers, roles, members, emojis and channels (https://gist.github.com/Gelbpunkt/751189ef40e8fdbe4edd0ad671bb3f19)
