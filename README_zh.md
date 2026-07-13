# FreeClaudeDesktop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![GUI: Iced](https://img.shields.io/badge/GUI-Iced-4b6cb7.svg?style=for-the-badge)](https://github.com/iced-rs/iced)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop 是跨平台的 Claude Desktop 啟動器與本機 API Proxy。它可將 Claude Desktop 連接到 OpenAI 相容或 Anthropic 相容的 AI Gateway，並預設只在本機運作。

[English](README.md)

## 功能

- 在 `127.0.0.1:3000` 提供 Claude Desktop 相容的本機 API Proxy。
- 支援 OpenAI 相容與 Anthropic 相容的上游服務。
- 支援模型探索、模型路由、推理設定與串流回應。
- 使用隔離的 Claude Desktop Profile，並可重新同步官方 Profile 的指定資料。
- 提供英文與繁體中文介面。
- 支援 Windows、macOS 與 Linux。

## 建置與執行

需求：

- 穩定版 Rust toolchain 與 Cargo
- Iced 與 `tray-icon` 所需的平台相依套件
- 用於啟動整合的 Claude Desktop

```bash
cargo test
cargo check
cargo build --release
cargo run
```

建立 Debian/Ubuntu 套件：

```bash
cargo install cargo-deb
cargo deb --locked
```

## 發佈

Release 由 GitHub Actions 手動執行，會從所選分支或 commit 建置 Windows、Linux 與 macOS 產物。GitHub Release 必須關聯 tag，因此 workflow 會在發佈時依 `Cargo.toml` 版本建立 tag；推送 tag 不會觸發發佈。
發佈資產包含 SHA-256 checksum 與 Sigstore provenance bundle。目前 Windows 與 macOS 產物未簽章，第一次開啟時可能出現作業系統安全警告。
重跑既有版本時，workflow 會確認 tag 仍指向相同 commit，避免以不同建置覆蓋已發佈資產。

## Project Links

- [架構文件](ARCHITECTURE.md)
- [Releases](https://github.com/mushroomTW/FreeClaudeDesktop/releases)
- [Issue Tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## 安全性

- 請勿在 Issue 或日誌中提交 API Key、工作階段 Cookie 或完整本機設定檔。
- Proxy 預設只綁定 loopback；如需公開網路存取，應先設計適當的驗證與網路安全措施。
- 發佈前請檢查產生的 Claude Desktop 設定內容。

## License

FreeClaudeDesktop 採用 [MIT License](LICENSE) 授權。
