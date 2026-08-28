# FreeClaudeDesktop

<p align="center">
  <img src="icon.png" alt="FreeClaudeDesktop icon" width="128" />
</p>

<!-- badge source: LICENSE (MIT) -->
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](LICENSE)
<!-- badge source: Cargo.toml (workspace.package.rust-version) -->
[![Rust 1.97.1](https://img.shields.io/badge/Rust-1.97.1-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop is a cross-platform command-line launcher and local API proxy for Claude Desktop. It lets Claude Desktop talk to any OpenAI-compatible or Anthropic-compatible gateway while keeping the proxy bound to `127.0.0.1`.

> [繁體中文](README_zh.md)

## Screenshots

> Click a section to expand — 4 Console pages are collapsed by default to save space.

<details>
<summary><strong>Connection Settings</strong> — API Provider, Gateway URL, API Key and Claude path detection</summary>
<br>

<p align="center">
  <img src="docs/images/console-connection.png" alt="FreeClaude Console - Connection Settings" width="800" />
</p>

</details>

<details>
<summary><strong>Model Settings</strong> — Alias routing (Sonnet / Opus / Haiku) and discovered models</summary>
<br>

<p align="center">
  <img src="docs/images/console-model-settings.png" alt="FreeClaude Console - Model Settings" width="800" />
</p>

</details>

<details>
<summary><strong>Request Optimization &amp; Tools</strong> — Quota mock, prefix detection, title/suggestion skip</summary>
<br>

<p align="center">
  <img src="docs/images/console-optimization.png" alt="FreeClaude Console - Request Optimization & Tools" width="800" />
</p>

</details>

<details>
<summary><strong>Advanced Settings</strong> — API call logging, transport protocol and thinking mode</summary>
<br>

<p align="center">
  <img src="docs/images/console-advanced.png" alt="FreeClaude Console - Advanced Settings" width="800" />
</p>

</details>

## Table of Contents

- [Screenshots](#screenshots)
- [About](#about)
- [Features](#features)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Configuration](#configuration)
- [CLI Reference](#cli-reference)
- [Proxy API](#proxy-api)
- [Architecture & Project Structure](#architecture--project-structure)
- [Extensions & Local Optimizations](#extensions--local-optimizations)
- [Development](#development)
- [Security](#security)
- [Limitations](#limitations)
- [Uninstall](#uninstall)
- [Project Links](#project-links)
- [License](#license)

## About

Claude Desktop speaks the Anthropic Messages API. FreeClaudeDesktop sits between Claude Desktop and your chosen gateway:

- Accepts Anthropic requests from Claude Desktop (`/v1/messages`, `/v1/models`).
- Translates them to OpenAI Chat Completions, forwards to the configured gateway, and streams the response back as Anthropic SSE/JSON.
- Applies local optimizations (quota probe mock, title/suggestion skips, prefix detection) so common UI probes never hit the gateway.

The proxy is the protocol boundary. Settings and secrets stay on your machine: non-secret config in the local settings store, API keys in the OS keyring (never returned by the Dashboard API).

For a deeper walk-through, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Features

- **Local proxy on `127.0.0.1:3000`** — Claude Desktop-compatible endpoint, loopback-only by default.
- **Gateway agnostic** — works with OpenAI-compatible and Anthropic-compatible upstreams (`auto` / `bearer` / `x-api-key` / `sso` auth schemes).
- **Full model discovery** — fetches `/v1/models` from the gateway, normalizes metadata, and publishes every model in the Claude Desktop picker with per-model visibility toggles.
- **Model routing & reasoning** — maps `claude-opus-5[0]` / `claude-sonnet-4[0]` / `claude-haiku-4[0]`-style aliases to real gateway models; translates `thinking.budget_tokens` → `reasoning_effort` and replays reasoning as native thinking blocks or `<antThinking>` text.
- **Isolated Claude Desktop profile** — keeps a dedicated profile for the proxied setup and can re-sync selected data from the official profile.
- **Cross-platform** — Windows (Task Scheduler + Registry Run key), macOS (LaunchAgent), Linux (systemd user service); x64 and ARM64 binaries via npm optional dependencies.
- **Host Companion Daemon** — maintains the `/companion` WebSocket for Dashboard ↔ host RPC.

## Quick Start

Prerequisites: [Claude Desktop](https://claude.ai/download) and Node.js with npm. The npm package auto-selects the binary matching your OS and CPU architecture.

```bash
npm install -g @mushroomtw/freeclaudedesktop
freecd install
freecd dashboard
```

1. `freecd install` sets up the isolated profile, writes the proxied Claude Desktop config, starts the native proxy, and enables autostart at login (use `--no-autostart` to opt out).
2. `freecd dashboard` opens the same-origin Web Dashboard at `http://127.0.0.1:3000/dashboard` — configure **Gateway URL**, **Auth Scheme**, and **API Key** there. `freecd start` alone only starts the proxy.

Verify the proxy is up:

```bash
curl http://127.0.0.1:3000/healthz
# {"status":"ok"}  (or similar JSON)
```

Then launch Claude Desktop — the model picker will list the gateway's models (e.g. `claude-opus-5[0]`).

> [!TIP]
> For the best Claude Desktop experience, use upstream models with multimodal input and ≥ 200K context. Text-only or small-context models still work, but images, long conversations, file handling, and tool-heavy flows may be limited.

## Installation

### Option A — npm (recommended)

```bash
npm install -g @mushroomtw/freeclaudedesktop
freecd install                # autostart enabled by default
freecd install --no-autostart # without autostart
```

Package: [`@mushroomtw/freeclaudedesktop@1.0.2`](packages/freeclaudedesktop/package.json) — ships `freecd` / `freeclaude` bins and six platform optional dependencies (`darwin-arm64/x64`, `linux-arm64/x64`, `win32-arm64/x64`).

### Option B — Build from source

Requires the [Rust 1.97.1 toolchain](https://www.rust-lang.org/tools/install) (`Cargo.toml` → `workspace.package.rust-version`).

```bash
git clone https://github.com/mushroomTW/FreeClaudeDesktop.git
cd FreeClaudeDesktop
cargo build --release
# macOS / Linux
./target/release/freeclaude install
# Windows (PowerShell)
.\target\release\freeclaude.exe install
```

Cargo builds the native CLI as `freeclaude`; the npm wrapper exposes it as `freecd`. All workspace binaries:

```bash
cargo build --release   # builds freeclaude (cli) + freeclaude-proxy
```

## Configuration

All settings are edited through the Web Dashboard (`/dashboard` → `/settings` API). API keys are stored in the OS keyring and never returned by `GET /settings`.

### Environment variables

| Variable | Purpose | Default | Where read |
|---|---|---|---|
| `FREECLAUDE_PROXY_PORT` | Proxy listen port | `3000` (`core/src/core/constants.rs:DEFAULT_PORT`) | `cli/src/main.rs:proxy_port()`, `proxy/src/main.rs` |
| `FREECLAUDE_PROXY_URL` | Override URL used by `freecd status` health check | `http://127.0.0.1:{PORT}` | `cli/src/main.rs:print_proxy_status()` |

### Dashboard settings (persisted via `core/src/core/config.rs`)

- **Gateway**: Base URL, auth scheme (`auto` / `bearer` / `x-api-key` / `sso`), transport type, proxy auth token.
- **Models**: per-alias route table (`real_model_routes`), reasoning effort routes, discovered models, `supports1m` / `prefer1m` / visibility overrides, `reasoning_replay_mode`.
- **Optimizations**: toggles for quota mock, prefix detection, title/suggestion skip, filepath extraction, web tools, API call logging.
- **Desktop**: custom Claude path, active port.
- **UI**: theme (`light`/`dark`), language.

## CLI Reference

Run `freecd --help` or `freecd <command> --help` for full help. Summary (`cli/src/cli_args.rs`):

```
freecd install [--no-autostart]
freecd start
freecd stop
freecd status
freecd configure              # opens http://127.0.0.1:{port}/dashboard
freecd dashboard              # alias for configure
freecd launch-claude
freecd restore                # restore official Claude config
freecd purge --yes            # remove app data (requires --yes)
freecd update [--check]
freecd uninstall
freecd autostart enable|disable|status
```

Common flows:

```bash
freecd start
freecd status
freecd stop

freecd autostart enable
freecd autostart status
freecd autostart disable

# Check for a newer GitHub Release without changing local install
freecd update --check
```

`start` waits for `GET /healthz` to succeed (up to ~5 s) before reporting success. Autostart backends: Windows Task Scheduler / Registry Run key, macOS LaunchAgent, Linux systemd user service (`cli/src/runtime/autostart.rs`).

## Proxy API

All routes are served from `http://127.0.0.1:{port}` (`proxy/src/server/router.rs`). CORS allows same-origin dashboard requests only.

| Method | Path | Description |
|---|---|---|
| `GET` | `/` | Root / landing |
| `GET` | `/healthz` | Health check — used by `freecd start` |
| `GET` | `/dashboard` | Web Dashboard HTML |
| `GET` | `/dashboard.css` | Dashboard stylesheet |
| `GET` | `/dashboard.js` | Dashboard script |
| `GET` | `/assets/icon.png` | App icon |
| `GET` | `/settings` | Non-secret settings (API key omitted) |
| `POST` | `/settings` | Update settings |
| `GET` | `/status` | Runtime status |
| `POST` | `/rpc` | Dashboard RPC → Companion Daemon (`GetStatus`, `DetectClaude`, `ApplySettings`, `FetchModels`, …) |
| `GET` | `/companion` | WebSocket — first message must include `requestId` |
| `POST` | `/v1/messages` | Anthropic Messages API (proxied to gateway) |
| `GET` | `/v1/models` | Normalized model list (Claude-facing aliases) |

Body limit: `16 MiB` (`core/src/core/constants.rs:MAX_PROXY_BODY_BYTES`). Gateway timeout: `60 s`.

Logs are written to `{local_app_data}/FreeClaudeDesktop/logs/` — `launcher.log` (daily rolling) and `api-calls.log` (10 MiB + 4 archives, `proxy/src/server/api_log.rs`). Enable API call logging from the Dashboard.

## Architecture & Project Structure

```
FreeClaudeDesktop/
├── core/        # free-claude-core — schemas, settings store, keyring, model routing, conversion
├── proxy/       # freeclaude-proxy — Axum routes, gateway forwarding, SSE conversion, dashboard
├── cli/         # freeclaude — install/lifecycle/profile/autostart/companion orchestration
├── packages/freeclaudedesktop/  # npm wrapper (bin/freecd) + platform optionalDependencies
└── docs/        # project documentation (ARCHITECTURE.md, EXTENSIONS_AND_SKILLS.md)
```

Crate dependency direction: `proxy` → `core`, `cli` → `core` (+ `proxy` for server handle). See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for runtime topology, message execution flow, model discovery/routing, and state ownership diagrams.

Key design notes:

- **Alias stability**: `claude-opus-5[0]`-style aliases are stable per model-list position; `supports1m` / `prefer1m` control the 1M-context variant without changing the ID.
- **Companion Daemon**: the CLI starts the host-side Companion Daemon to keep the `/companion` WebSocket alive.

## Extensions & Local Optimizations

Detailed in [docs/EXTENSIONS_AND_SKILLS.md](docs/EXTENSIONS_AND_SKILLS.md). Summary of toggles in Dashboard → Optimizations:

| Optimization | What it does |
|---|---|
| **Quota Mock** | Intercepts `max_tokens=1` quota probes and answers locally |
| **Prefix Detection** | Resolves common shell prefixes (`git`, `cargo`, `docker`, …) without an LLM call |
| **Title Generation Skip** | Returns fixed `"Conversation"` instead of calling the LLM for a chat title |
| **Suggestion Skip** | Returns empty follow-up suggestions locally |
| **Filepath Extraction** | Extracts file paths from command output via local regex |
| **Web Tools** | Executes `web_search` / `web_fetch` locally (configurable allowed schemes + private-network guard) |
| **API Call Logging** | When enabled, appends JSON Lines to `api-calls.log` |

## Development

Toolchain pinned to **Rust 1.97.1** (`rust-version` in `Cargo.toml`, enforced in `.github/workflows/ci.yml`).

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check
```

CI runs on `ubuntu-latest`, `macos-15`, `windows-latest`.

Release builds for six targets (`x86_64`/`aarch64` × Linux/macOS/Windows) via `scripts/pack-platform.mjs` → `dist/npm/`; publish with `pnpm@10.15.0` (`packageManager` in `package.json`).

SonarQube local analysis uses `sonar-project.properties` at the repo root (see `sonar-project.properties`).

## Security

- The proxy binds to loopback by default. Do not expose it to a LAN or the public internet without adding authentication and network controls.
- API keys are stored in the OS keyring (`keyring` crate) and never returned by `GET /settings` or the Dashboard API.
- Review the generated Claude Desktop configuration before distributing it.
- `web_fetch` can be restricted to `http`/`https` schemes and blocked from private networks (Dashboard → Web Fetch settings) to mitigate SSRF.
- Report security issues via the [issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues) — do not post credentials.

> **Disclaimer:** This project is not affiliated with, endorsed by, or supported by Anthropic. “Claude” and “Claude Desktop” are trademarks of their respective owners. This program coordinates third-party models; you are responsible for API costs, credentials, and data-sharing choices.

## Limitations

- Upstream models without multimodal input or with small context windows still work, but image, long-conversation, file, and tool-heavy workflows may be degraded (see Quick Start tip above).
- `reasoning_replay_mode` and `supports1m`/`prefer1m` depend on the gateway exposing the relevant capabilities via `/v1/models` and chat completions.
- Linux 1M-context runtime patch attempts are documented separately in `docs/linux/claude-desktop-1m-runtime-patch-attempts.md`.
- The proxy enforces a `16 MiB` request body limit and a `60 s` gateway timeout — configurable only via code (`core/src/core/constants.rs`).

## Uninstall

Clean up local state before removing the npm package:

```bash
freecd uninstall
npm uninstall -g @mushroomtw/freeclaudedesktop
```

`uninstall` stops the proxy and Companion Daemon, disables autostart, restores the official Claude config, and purges app data (`cli/src/main.rs:uninstall`).

## Project Links

- [Architecture](docs/ARCHITECTURE.md)
- [Extensions & Skills](docs/EXTENSIONS_AND_SKILLS.md)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## License

FreeClaudeDesktop is released under the [MIT License](LICENSE).
