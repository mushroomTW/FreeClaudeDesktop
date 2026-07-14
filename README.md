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

## Quick start

Prerequisite: Claude Desktop. The commands below download the matching prebuilt release and install the local `freeclaude` CLI; they do not require Rust, Cargo, Git, or a source checkout.

### macOS / Linux

```bash
curl -fsSL "https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download/install.sh" | sh
```

### Windows (PowerShell)

```powershell
irm "https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download/install.ps1" | iex
```

The stable installer URL resolves to the latest GitHub Release. It downloads the matching prebuilt binaries, validates their SHA-256 hash against the release `checksums.txt`, and adds `freeclaude` to your user PATH. It does **not** change Claude Desktop settings; run the following after reviewing the installation:

```text
freeclaude install
freeclaude configure
```

### Manual installation

Only use this route when building from source; it requires the stable [Rust toolchain](https://www.rust-lang.org/tools/install).

```bash
git clone https://github.com/mushroomTW/FreeClaudeDesktop.git
cd FreeClaudeDesktop
cargo install --path cli
cargo build --release -p freeclaude-proxy
```

`freeclaude install` uses the native proxy by default. Use `freeclaude install --runtime docker` if you explicitly want the Docker runtime.

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

The service is available at `http://127.0.0.1:3000`, including its Web Admin page. The image contains no API key or proxy token. Docker memory usage is capped at **4 GB** by default. To change it, copy `.env.example` to `.env` and set `FREECLAUDE_DOCKER_MEMORY_LIMIT` (for example, `512m` or `2g`).

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
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## Security

- Never include API keys, session cookies, or full local configuration files in issues or logs.
- The proxy is bound to loopback by default. Do not expose it publicly without designing appropriate authentication and network controls.
- Review generated Claude Desktop configuration before distributing it.

> **Disclaimer:** This project is not affiliated with, endorsed by, or supported by Anthropic. “Claude” and “Claude Desktop” are trademarks of their respective owners. This program coordinates third-party models; you are responsible for API/cloud-service costs, credentials, and data-sharing choices.

## License

FreeClaudeDesktop is released under the [MIT License](LICENSE).
