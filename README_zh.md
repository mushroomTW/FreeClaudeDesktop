# FreeClaudeDesktop

![FreeClaudeDesktop 圖標](icon.png)

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

## 快速開始

只需先安裝 Claude Desktop。以下指令會下載符合平台的預編譯 release 並安裝本機 `freeclaude` CLI；不需要 Rust、Cargo、Git 或原始碼工作區。

### macOS / Linux

```bash
curl -fsSL "https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download/install.sh" | sh
```

### Windows（PowerShell）

```powershell
irm "https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download/install.ps1" | iex
```

穩定安裝器 URL 會解析至最新 GitHub Release。安裝程式會下載符合平台的預編譯 binary、依 release 的 `checksums.txt` 驗證 SHA-256，並將 `freeclaude` 加入使用者 PATH；它**不會**修改 Claude Desktop 設定。請在確認安裝內容後，再執行：

```text
freeclaude install
freeclaude configure
```

### 手動安裝

只有需要從原始碼建置時才使用此方式；它需要穩定版 [Rust toolchain](https://www.rust-lang.org/tools/install)。

```bash
git clone https://github.com/mushroomTW/FreeClaudeDesktop.git
cd FreeClaudeDesktop
cargo build --release -p freeclaude -p freeclaude-proxy
# macOS / Linux
./target/release/freeclaude install
# Windows（PowerShell）
.\target\release\freeclaude.exe install
```

`freeclaude install` 預設使用原生 proxy；只有明確需要 Docker runtime 時才使用 `freeclaude install --runtime docker`。

## 以 pnpm 全域安裝

```bash
pnpm add -g @mushroomtw/freeclaudedesktop
freeclaude-proxy start
```

可使用 `freeclaude-proxy status`、`restart`、`admin` 與 `path` 管理本機服務；執行 `freeclaude-proxy purge` 可完整重設本機資料。解除安裝時會停止服務、還原 Claude 設定，並完整清除 FreeClaudeDesktop 擁有的設定、隔離 profile 與 OS keyring API key：

```bash
pnpm remove -g @mushroomtw/freeclaudedesktop
```

## 建置與執行

需求：

- 穩定版 Rust toolchain 與 Cargo。
- 用於啟動整合的 Claude Desktop。

```bash
cargo build --release -p freeclaude -p freeclaude-proxy
# macOS / Linux
./target/release/freeclaude start
# Windows（PowerShell）
.\target\release\freeclaude.exe start
```

## CLI 與本機管理

在開發工作區同時建置兩個 binary：

```bash
cargo build -p freeclaude -p freeclaude-proxy
```

管理原生 Proxy：

```bash
freeclaude start
freeclaude status
freeclaude stop
```

預設連接埠為 `3000`。設定 `FREECLAUDE_PROXY_PORT` 可使用其他本機連接埠；`start` 會等待 `/healthz` 成功後才回報完成。

`freeclaude configure` 會開啟同源的 `/admin` Web Admin 頁面。不需要登入或 Proxy Token，即可查看狀態與更新 Gateway 設定。API key 儲存在作業系統 keyring，Admin API 不會回傳它。

執行 `freeclaude start` 後，也可直接開啟 [http://127.0.0.1:3000/admin](http://127.0.0.1:3000/admin) 使用 Web Admin。

```text
GET  /healthz
GET  /admin/settings
POST /admin/settings
GET  /admin/status
POST /admin/rpc
WS   /companion           （首個訊息需要 requestId）
```

管理自動啟動與移除：

```bash
freeclaude autostart enable
freeclaude autostart status
freeclaude autostart disable
freeclaude uninstall --yes
freeclaude purge --yes
```

Windows 使用 Task Scheduler、macOS 使用 LaunchAgent、Linux 使用 systemd user service。

## Companion Daemon

每次啟動 Proxy 時，CLI 都會啟動宿主機上的 Companion Daemon，`--runtime docker` 也相同。它會維持本機 `/companion` WebSocket 連線，供 Claude Desktop RPC 使用。Docker 容器只包含 Proxy，因此 Companion Daemon 必須在宿主機執行。

## Docker

Docker Compose 用法、記憶體上限、安全注意事項與 CLI 管理指令，請見 [DOCKER.md](DOCKER.md)。

僅檢查是否有新版 GitHub Release、但不變更本機安裝：

```bash
freeclaude update --check
```

## 專案連結

- [架構](ARCHITECTURE.md)
- [擴充與本地技能功能介紹](EXTENSIONS_AND_SKILLS.md)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## 安全性

- 請勿在 Issue 或日誌中包含 API key、session cookie 或完整本機設定檔。
- Proxy 預設僅繫結於 loopback。除非已規劃適當的驗證與網路控管，否則不要公開對外。
- 散布前請檢視產生的 Claude Desktop 設定。

> **免責聲明：**與 Anthropic 無任何關聯，亦未獲得其認可或支持。「Claude」和「Claude Desktop」均為其各自所有者的商標。此程式負責協調第三方模型；您需自行承擔 API／雲端服務的費用、憑證以及資料共享選擇。

## 授權

FreeClaudeDesktop 採用 [MIT License](LICENSE)。
