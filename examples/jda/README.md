# JDA Example

This repository showcases the usage of the gateway proxy with JDA. It uses Spring-Boot as the bootstrap environment and
uses ByteBuddy for hacking around a JDA 4 limitation. This repository requires Java 8 but is compatible with newer
versions.

Log is set to `TRACE` for JDA so payloads are visible. To start, you need to configure the `application.yml` file under
the resources folder. You can then run using gradle: `./gradlew bootRun`.
