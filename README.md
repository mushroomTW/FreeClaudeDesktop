# FreeClaudeDesktop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop is a cross-platform command-line launcher and local API proxy for Claude Desktop. It connects Claude Desktop to OpenAI-compatible and Anthropic-compatible AI gateways while keeping the proxy bound to the local machine.

[繁體中文](README_zh.md)

## Features

- Runs a local Claude Desktop-compatible API proxy on `127.0.0.1:3000`.
- Supports OpenAI-compatible and Anthropic-compatible upstream services.
- Discovers models and supports model routing, reasoning settings, and streaming responses.
- Keeps an isolated Claude Desktop profile and supports re-syncing selected data from the official profile.
- Provides a browser-based Web Admin interface in English and Traditional Chinese.
- Supports Windows, macOS, and Linux.

## Build and Run

Requirements:

- Stable Rust toolchain with Cargo
- Claude Desktop for launcher integration

```bash
cargo test
cargo check
cargo build --release
cargo run
```

Install the CLI from a source checkout:

```bash
cargo install --path cli
```

CI/CD workflows are intentionally not included. Build and test releases locally with the commands above.

## CLI and local administration

Build both binaries in a development checkout:

```bash
cargo build --bin freeclaude --bin freeclaude-proxy
```

Manage the native proxy:

```bash
freeclaude start
freeclaude status
freeclaude stop
```

The default port is `3000`. Set `FREECLAUDE_PROXY_PORT` to use another local port; `start` waits for `/healthz` before reporting success.

`freeclaude configure` opens the same-origin Web Admin page at `/admin`. Enter the local proxy token to inspect status and update gateway settings. API keys are stored in the operating-system keyring and are never returned by the Admin API.

After `freeclaude start`, open [http://127.0.0.1:3000/admin](http://127.0.0.1:3000/admin) to use Web Admin directly.

```text
GET  /healthz
GET  /admin/settings      (Bearer proxy token required)
POST /admin/settings      (Bearer proxy token required)
GET  /admin/status        (Bearer proxy token required)
POST /admin/rpc           (Bearer proxy token required)
WS   /companion           (first message requires token and requestId)
```

Manage startup and removal:

```bash
freeclaude autostart enable
freeclaude autostart status
freeclaude autostart disable
freeclaude uninstall --yes
```

Windows uses Task Scheduler, macOS uses a LaunchAgent, and Linux uses a systemd user service.

## Companion daemon

The CLI starts a host-side Companion Daemon whenever the proxy starts, including with `--runtime docker`. It maintains the local `/companion` WebSocket connection used for Claude Desktop RPC. The daemon runs on the host because the Docker container contains only the proxy.

## Docker

Docker Compose maps the proxy only to localhost:

```bash
docker compose up --build
```

The service is available at `http://127.0.0.1:3000`, including its Web Admin page. The image contains no API key or proxy token.

Run the same lifecycle through the CLI from the repository directory (or set `FREECLAUDE_COMPOSE_FILE` to the compose-file path):

```bash
freeclaude install --runtime docker
freeclaude status --runtime docker
freeclaude stop --runtime docker
freeclaude start --runtime docker
freeclaude update --runtime docker
freeclaude uninstall --runtime docker --yes --purge-image
```

Check for a newer GitHub Release without changing the local installation:

```bash
freeclaude update --check
```

## Project Links

- [Architecture](ARCHITECTURE.md)
- [Releases](https://github.com/mushroomTW/FreeClaudeDesktop/releases)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## Security

- Never include API keys, session cookies, or full local configuration files in issues or logs.
- The proxy is bound to loopback by default. Do not expose it publicly without designing appropriate authentication and network controls.
- Review generated Claude Desktop configuration before distributing it.

## License

FreeClaudeDesktop is released under the [MIT License](LICENSE).
