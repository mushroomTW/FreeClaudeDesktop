# FreeClaudeDesktop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Edition 2024](https://img.shields.io/badge/edition-2024-3776ab.svg?style=for-the-badge)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![GUI: Iced](https://img.shields.io/badge/GUI-Iced-4b6cb7.svg?style=for-the-badge)](https://github.com/iced-rs/iced)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)
[![CI](https://github.com/mushroomTW/FreeClaudeDesktop/actions/workflows/ci.yml/badge.svg?style=for-the-badge)](https://github.com/mushroomTW/FreeClaudeDesktop/actions/workflows/ci.yml)

FreeClaudeDesktop is a cross-platform desktop launcher and local API proxy for Claude Desktop. It connects Claude Desktop to OpenAI-compatible and Anthropic-compatible AI gateways while keeping the proxy bound to the local machine.

[繁體中文](README_zh.md)

## Features

- Runs a local Claude Desktop-compatible API proxy on `127.0.0.1:3000`.
- Supports OpenAI-compatible and Anthropic-compatible upstream services.
- Discovers models and supports model routing, reasoning settings, and streaming responses.
- Keeps an isolated Claude Desktop profile and supports re-syncing selected data from the official profile.
- Provides English and Traditional Chinese user interfaces.
- Supports Windows, macOS, and Linux.

## Build and Run

Requirements:

- Stable Rust toolchain with Cargo
- Platform dependencies required by Iced and `tray-icon`
- Claude Desktop for launcher integration

```bash
cargo test
cargo check
cargo build --release
cargo run
```

To build a Debian/Ubuntu package:

```bash
cargo install cargo-deb
cargo deb --locked
```

## Releases

Releases are created manually from GitHub Actions. The workflow builds Windows, Linux, and macOS artifacts from the selected branch or commit. A GitHub Release always has a tag, so the workflow creates the version tag from `Cargo.toml` when publishing; pushing a tag does not trigger a release. Re-running an existing version is allowed only when its tag points to the same commit, preventing assets from being replaced with a different build.

Published assets include SHA-256 checksums and Sigstore provenance bundles. Current Windows and macOS builds are unsigned, so their operating systems may show a first-run security warning.

## Project Links

- [Source code](https://github.com/mushroomTW/FreeClaudeDesktop)
- [Releases](https://github.com/mushroomTW/FreeClaudeDesktop/releases)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)
- [Architecture](ARCHITECTURE.md)
- [Reference project: free-claude-code](https://github.com/Alishahryar1/free-claude-code)

## Security

- Never include API keys, session cookies, or full local configuration files in issues or logs.
- The proxy is bound to loopback by default. Do not expose it publicly without designing appropriate authentication and network controls.
- Review generated Claude Desktop configuration before distributing it.

## License

FreeClaudeDesktop is released under the [MIT License](LICENSE).
