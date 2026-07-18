# Docker Runtime Guide

This guide explains how to run the FreeClaudeDesktop Proxy with Docker Compose. The Docker runtime is optional; the native runtime is recommended for typical use.

## Prerequisites

- Docker Engine or Docker Desktop, including Docker Compose v2
- `compose.yaml`, `Dockerfile`, and `.env.example` in the project root

## Quick start

Run this command from the project root:

```bash
docker compose up --build
```

The proxy is published only on the local machine at `http://127.0.0.1:3000`. Web Admin does not have a sign-in flow. Do not modify `compose.yaml` to expose this port to a LAN or the public Internet.

Stop the service with:

```bash
docker compose down
```

## Memory limit

The default limit is **4 GB**. Copy `.env.example` to `.env`, then set `FREECLAUDE_DOCKER_MEMORY_LIMIT`:

```dotenv
FREECLAUDE_DOCKER_MEMORY_LIMIT=2g
```

Docker Compose accepts memory values such as `512m`, `1g`, and `2g`. Recreate the container after changing the value:

```bash
docker compose up --build --force-recreate
```

## Managing the Docker runtime with the CLI

From the project root, run:

```bash
freeclaude install --runtime docker
freeclaude status --runtime docker
freeclaude stop --runtime docker
freeclaude start --runtime docker
freeclaude update --runtime docker
freeclaude uninstall --runtime docker --yes --purge-image
```

If the Compose file is stored elsewhere, set `FREECLAUDE_COMPOSE_FILE` to its absolute path.
Docker runtime currently uses port `3000` only. Do not set `FREECLAUDE_PROXY_PORT` when using it; use the native runtime if a custom port is required.

The prebuilt `freeclaude` release contains only the executables, not the Docker build context. Docker commands therefore require a checkout or another directory containing this project's `compose.yaml` and `Dockerfile`.

## Companion daemon

The Docker container contains only the proxy. Each time the CLI starts the Docker proxy, it also starts the Companion Daemon on the host. The daemon maintains the local `/companion` WebSocket connection used for Claude Desktop RPC.
