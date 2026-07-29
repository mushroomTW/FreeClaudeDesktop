# FreeClaudeDesktop

<p align="center">
  <img src="icon.png" alt="FreeClaudeDesktop icon" />
</p>

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust 1.97.1](https://img.shields.io/badge/Rust-1.97.1-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop is a cross-platform command-line launcher and local API proxy for Claude Desktop. It connects Claude Desktop to OpenAI-compatible and Anthropic-compatible AI gateways while keeping the proxy bound to the local machine.

[繁體中文](README_zh.md)

## Features

- Runs a local Claude Desktop-compatible API proxy on `127.0.0.1:3000`.
- Supports OpenAI-compatible and Anthropic-compatible upstream services.
- Discovers every model returned by the upstream API's `/v1/models` endpoint and makes them available in the Claude Desktop model picker, with configurable visibility.
- Supports model routing, reasoning settings, and streaming responses.
- Keeps an isolated Claude Desktop profile and supports re-syncing selected data from the official profile.
- Supports Windows, macOS, and Linux.

For the best Claude Desktop experience, use upstream models with multimodal input support and a context window of at least 200K tokens. Models with smaller context windows or text-only capabilities may still work, but images, long conversations, files, and tool-heavy workflows can be limited.

## Quick start

Prerequisite: Claude Desktop and Node.js with npm. Install the published package globally; npm automatically selects the binary package for your operating system and CPU architecture.

```bash
npm install -g @mushroomtw/freeclaudedesktop
freecd install
freecd dashboard
```

Use the Web Dashboard to configure the Gateway URL, API key, and model settings. `freecd start` only starts the proxy; `freecd install` also sets up the local integration and default autostart.

### Manual installation

Only use this route when building from source; it requires the [Rust 1.97.1 toolchain](https://www.rust-lang.org/tools/install). Cargo builds the native CLI as `freeclaude`; the npm package exposes it through `freecd`.

```bash
git clone https://github.com/mushroomTW/FreeClaudeDesktop.git
cd FreeClaudeDesktop
cargo build --release
# macOS / Linux
./target/release/freeclaude install
# Windows (PowerShell)
.\target\release\freeclaude.exe install
```

`freecd install` uses the native proxy and enables startup at login by default. Use `freecd install --runtime docker` if you explicitly want the Docker runtime, or add `--no-autostart` to either installation mode if you do not want automatic startup.

## Build and Run

Requirements:

- Rust 1.97.1 with Cargo
- Claude Desktop for launcher integration

```bash
cargo build --release
# macOS / Linux
./target/release/freeclaude start
# Windows (PowerShell)
.\target\release\freeclaude.exe start
```

## CLI and local management

Build all workspace binaries in a development checkout:

```bash
cargo build --release
```

Manage the native proxy:

```bash
freecd start
freecd status
freecd stop
```

The default port is `3000`. Set `FREECLAUDE_PROXY_PORT` to use another local port; `start` waits for `/healthz` before reporting success.

`freecd configure` opens the same-origin Web Dashboard page at `/dashboard`. No sign-in or proxy token is required. API keys are stored in the operating-system keyring and are never returned by the Dashboard API.

After `freecd start`, open [http://127.0.0.1:3000/dashboard](http://127.0.0.1:3000/dashboard) to use Web Dashboard directly.

```text
GET  /healthz
GET  /settings
POST /settings
GET  /status
POST /rpc
WS   /companion           (first message requires requestId)
```

Manage startup and removal:

```bash
freecd autostart enable
freecd autostart status
freecd autostart disable
```

Startup at login is enabled by default when running `freecd install`. Pass `--no-autostart` during installation to opt out. Windows uses Task Scheduler, macOS uses a LaunchAgent, and Linux uses a systemd user service.

## Uninstall

Before removing the npm package, clean up local state:

```bash
freecd uninstall
npm uninstall -g @mushroomtw/freeclaudedesktop
```

## Companion daemon

The CLI starts a host-side Companion Daemon whenever the proxy starts, including with `--runtime docker`. It maintains the local `/companion` WebSocket connection used for Claude Desktop RPC. The daemon runs on the host because the Docker container contains only the proxy.

## Docker

See [DOCKER.md](DOCKER.md) for Docker Compose usage, memory limits, security notes, and CLI lifecycle commands.

Check for a newer GitHub Release without changing the local installation:

```bash
freecd update --check
```

## Project Links

- [Architecture](ARCHITECTURE.md)
- [Extensions & Skills](EXTENSIONS_AND_SKILLS.md)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## Security

- The proxy is bound to loopback by default. Do not expose it publicly without designing appropriate authentication and network controls.
- Review generated Claude Desktop configuration before distributing it.

> **Disclaimer:** This project is not affiliated with, endorsed by, or supported by Anthropic. “Claude” and “Claude Desktop” are trademarks of their respective owners. This program coordinates third-party models; you are responsible for API/cloud-service costs, credentials, and data-sharing choices.

## License

FreeClaudeDesktop is released under the [MIT License](LICENSE).
