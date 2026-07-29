# FreeClaudeDesktop

<p align="center">
  <img src="icon.png" alt="FreeClaudeDesktop 圖標" />
</p>

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=for-the-badge)](https://opensource.org/licenses/MIT)
[![Rust 1.97.1](https://img.shields.io/badge/Rust-1.97.1-000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![HTTP: Axum](https://img.shields.io/badge/HTTP-Axum-6d3f8c.svg?style=for-the-badge)](https://github.com/tokio-rs/axum)
[![Runtime: Tokio](https://img.shields.io/badge/runtime-Tokio-4c8eda.svg?style=for-the-badge)](https://tokio.rs/)

FreeClaudeDesktop 是跨平台的命令列啟動器與 Claude Desktop 本機 API Proxy。它讓 Claude Desktop 能連接 OpenAI 相容與 Anthropic 相容的 AI Gateway，同時將 Proxy 限制在本機。

[English](README.md)

## 功能

- 在 `127.0.0.1:3000` 提供 Claude Desktop 相容的本機 API Proxy。
- 支援 OpenAI 相容與 Anthropic 相容的上游服務。
- 探索上游 API `/v1/models` 端點回傳的所有模型，並讓這些模型顯示於 Claude Desktop 的模型選擇器；亦可個別控制是否顯示。
- 支援模型路由、推理設定與串流回應。
- 使用隔離的 Claude Desktop Profile，並可重新同步官方 Profile 的選定資料。
- 支援 Windows、macOS 與 Linux。

為獲得較完整的 Claude Desktop 使用體驗，建議上游模型支援多模態輸入，且上下文視窗至少為 200K tokens。上下文較小或僅支援文字的模型仍可能運作，但圖片、長對話、檔案及大量工具呼叫等情境可能受限。

## 快速開始

請先安裝 Claude Desktop，以及含 npm 的 Node.js。全域安裝已發布的 npm 套件後，npm 會自動選擇符合作業系統與 CPU 架構的 binary 套件。

```bash
npm install -g @mushroomtw/freeclaudedesktop
freecd install
freecd dashboard
```

請使用 Web 控制台設定 Gateway URL、API key 與模型。`freecd start` 只會啟動 Proxy；`freecd install` 會另外完成本機整合並預設啟用自動啟動。

### 手動安裝

只有需要從原始碼建置時才使用此方式；它需要 [Rust 1.97.1 toolchain](https://www.rust-lang.org/tools/install)。Cargo 建置的原生 CLI 名稱為 `freeclaude`；npm 套件則提供 `freecd` 入口。

```bash
git clone https://github.com/mushroomTW/FreeClaudeDesktop.git
cd FreeClaudeDesktop
cargo build --release
# macOS / Linux
./target/release/freeclaude install
# Windows（PowerShell）
.\target\release\freeclaude.exe install
```

`freecd install` 預設使用原生 Proxy，並預設啟用登入後自動啟動。只有明確需要 Docker runtime 時才使用 `freecd install --runtime docker`；若不希望自動啟動，兩種安裝模式都可加上 `--no-autostart`。

## 建置與執行

需求：

- Rust 1.97.1 與 Cargo。
- 用於啟動整合的 Claude Desktop。

```bash
cargo build --release
# macOS / Linux
./target/release/freeclaude start
# Windows（PowerShell）
.\target\release\freeclaude.exe start
```

## CLI 與本機管理

在開發工作區建置 workspace 中的所有 binary：

```bash
cargo build --release
```

管理原生 Proxy：

```bash
freecd start
freecd status
freecd stop
```

預設連接埠為 `3000`。設定 `FREECLAUDE_PROXY_PORT` 可使用其他本機連接埠；`start` 會等待 `/healthz` 成功後才回報完成。

`freecd configure` 會開啟同源的 `/dashboard` Web 控制台頁面。不需要登入或 Proxy Token，即可查看狀態與更新 Gateway 設定。API key 儲存在作業系統 keyring，控制台 API 不會回傳它。

執行 `freecd start` 後，也可直接開啟 [http://127.0.0.1:3000/dashboard](http://127.0.0.1:3000/dashboard) 使用 Web 控制台。

```text
GET  /healthz
GET  /settings
POST /settings
GET  /status
POST /rpc
WS   /companion           （首個訊息需要 requestId）
```

管理自動啟動與移除：

```bash
freecd autostart enable
freecd autostart status
freecd autostart disable
```

執行 `freecd install` 時預設會啟用登入後自動啟動；若要停用此預設行為，請在安裝時加上 `--no-autostart`。Windows 使用 Task Scheduler、macOS 使用 LaunchAgent、Linux 使用 systemd user service。

## 解除安裝

移除 npm 套件前，請先清理本機狀態：

```bash
freecd uninstall
npm uninstall -g @mushroomtw/freeclaudedesktop
```

## Companion Daemon

每次啟動 Proxy 時，CLI 都會啟動宿主機上的 Companion Daemon，`--runtime docker` 也相同。它會維持本機 `/companion` WebSocket 連線，供 Claude Desktop RPC 使用。Docker 容器只包含 Proxy，因此 Companion Daemon 必須在宿主機執行。

## Docker

Docker Compose 用法、記憶體上限、安全注意事項與 CLI 管理指令，請見 [DOCKER.md](DOCKER.md)。

僅檢查是否有新版 GitHub Release、但不變更本機安裝：

```bash
freecd update --check
```

## 專案連結

- [架構](ARCHITECTURE.md)
- [擴充與本地技能功能介紹](EXTENSIONS_AND_SKILLS.md)
- [Issue tracker](https://github.com/mushroomTW/FreeClaudeDesktop/issues)

## 安全性

- Proxy 預設僅繫結於 loopback。除非已規劃適當的驗證與網路控管，否則不要公開對外。
- 散布前請檢視產生的 Claude Desktop 設定。

> **免責聲明：**與 Anthropic 無任何關聯，亦未獲得其認可或支持。「Claude」和「Claude Desktop」均為其各自所有者的商標。此程式負責協調第三方模型；您需自行承擔 API／雲端服務的費用、憑證以及資料共享選擇。

## 授權

FreeClaudeDesktop 採用 [MIT License](LICENSE)。
