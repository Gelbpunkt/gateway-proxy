# Twilight Example

This is a very minimal example of how to use the gateway-proxy together with twilight's http-proxy in a single twilight bot.

Logging is set to DEBUG by default to showcase that heartbeating is working and payloads are properly formatted.

For this to work, run the http-proxy on port 8080 and the gateway-proxy on port 7878.

Export the token under the `TOKEN` environment variable before running the bot.
