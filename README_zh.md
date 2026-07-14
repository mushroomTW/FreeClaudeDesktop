# FreeClaudeDesktop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop 是跨平台的命令列啟動器與 Claude Desktop 本機 API Proxy。它讓 Claude Desktop 能連接 OpenAI 相容與 Anthropic 相容的 AI Gateway，同時將 Proxy 限制在本機。

[English](README.md)

## 功能

- 在 `127.0.0.1:3000` 提供 Claude Desktop 相容的本機 API Proxy。
- 支援 OpenAI 相容與 Anthropic 相容的上游服務。
- 支援模型探索、模型路由、推理設定與串流回應。
- 使用隔離的 Claude Desktop Profile，並可重新同步官方 Profile 的選定資料。
- 提供繁體中文與英文的瀏覽器 Web Admin。
- 支援 Windows、macOS 與 Linux。

## 建置與執行

需求：

- 穩定版 Rust toolchain 與 Cargo。
- 用於啟動整合的 Claude Desktop。

```bash
cargo test
cargo check
cargo build --release
cargo run
```

從原始碼工作區安裝 CLI：

```bash
cargo install --path cli
```

專案刻意不包含 CI/CD workflow；請使用上述命令在本機建置與測試發行版本。

## CLI 與本機管理

在開發工作區同時建置兩個 binary：

```bash
cargo build --bin freeclaude --bin freeclaude-proxy
```

管理原生 Proxy：

```bash
freeclaude start
freeclaude status
freeclaude stop
```

預設連接埠為 `3000`。設定 `FREECLAUDE_PROXY_PORT` 可使用其他本機連接埠；`start` 會等待 `/healthz` 成功後才回報完成。

`freeclaude configure` 會開啟同源的 `/admin` Web Admin 頁面。輸入本機 proxy token 後，即可查看狀態與更新 Gateway 設定。API key 儲存在作業系統 keyring，Admin API 不會回傳它。

執行 `freeclaude start` 後，也可直接開啟 [http://127.0.0.1:3000/admin](http://127.0.0.1:3000/admin) 使用 Web Admin。

```text
GET  /healthz
GET  /admin/settings      （需要 Bearer proxy token）
POST /admin/settings      （需要 Bearer proxy token）
GET  /admin/status        （需要 Bearer proxy token）
POST /admin/rpc           （需要 Bearer proxy token）
WS   /companion           （首個訊息需要 token 與 requestId）
```

管理自動啟動與移除：

```bash
freeclaude autostart enable
freeclaude autostart status
freeclaude autostart disable
freeclaude uninstall --yes
```

Windows 使用 Task Scheduler、macOS 使用 LaunchAgent、Linux 使用 systemd user service。

## Companion Daemon

每次啟動 Proxy 時，CLI 都會啟動宿主機上的 Companion Daemon，`--runtime docker` 也相同。它會維持本機 `/companion` WebSocket 連線，供 Claude Desktop RPC 使用。Docker 容器只包含 Proxy，因此 Companion Daemon 必須在宿主機執行。

## Docker

Docker Compose 僅將 Proxy 映射到 localhost：

```bash
docker compose up --build
```

服務與 Web Admin 位於 `http://127.0.0.1:3000`。image 不包含 API key 或 proxy token。

在專案目錄中透過 CLI 管理 Docker runtime；若 Compose 檔位於其他位置，請設定 `FREECLAUDE_COMPOSE_FILE`：

```bash
freeclaude install --runtime docker
freeclaude status --runtime docker
freeclaude stop --runtime docker
freeclaude start --runtime docker
freeclaude update --runtime docker
freeclaude uninstall --runtime docker --yes --purge-image
```

僅檢查是否有新版 GitHub Release、但不變更本機安裝：

```bash
freeclaude update --check
```

## 專案連結

- [架構](ARCHITECTURE.md)
- [Releases](https://github.com/mushroomTW/FreeClaudeDesktop/releases)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## 安全性

- 請勿在 Issue 或日誌中包含 API key、session cookie 或完整本機設定檔。
- Proxy 預設僅繫結於 loopback。除非已規劃適當的驗證與網路控管，否則不要公開對外。
- 散布前請檢視產生的 Claude Desktop 設定。

## 授權

FreeClaudeDesktop 採用 [MIT License](LICENSE)。
