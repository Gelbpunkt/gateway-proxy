# gateway-proxy

> This is a very hacky project, so it might stop working if Discord changes their API core. This is unlikely, but keep that in mind while using the proxy.

This is a proxy for Discord gateway connections - clients can connect to this proxy instead of the Discord Gateway and interact with it just like they would with the Discord Gateway.

The proxy connects to Discord instead of the client - allowing zero-downtime client restarts. The proxy won't invalidate your sessions or disconnect you (exceptions below).

## How?

It connects to Discord and simply relays all events to the client that sends an `IDENTIFY` payload with a shard ID that is managed by the proxy.

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

## Known Issues / TODOs

- Sequence numbers are very wrong
- It doesn't allow for RESUMEs, just re-identify. Resumes will result in a session invalidation payload to enforce this
- Some assumptions about state are being made that shouldn't be to ensure safety
