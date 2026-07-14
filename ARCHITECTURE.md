# 架構

FreeClaudeDesktop 是跨平台 Rust 專案，由宿主機 CLI/companion 與本機 proxy 組成。它不再包含 Iced 視窗、系統匣或原生 GUI 安裝包。

```text
freeclaude CLI + companion（宿主機）
  ├── 管理 Claude Desktop 設定、系統 keyring 與開機自啟動
  └── 管理 native 或 Docker proxy runtime

freeclaude-proxy
  ├── Axum /v1/messages、/v1/models、/admin/*、/healthz
  ├── 同源 Web Admin
  └── 已驗證 companion WebSocket 管理通道
```

## Workspace

| 元件 | 位置 | 職責 |
| --- | --- | --- |
| `free-claude-core` | `core/` | 設定 schema、Claude/OpenAI 轉換、模型路由、Claude Desktop 設定交易與跨平台啟動。 |
| `freeclaude-proxy` | `proxy/` | Axum proxy、串流轉換、管理頁與 companion RPC 端點。 |
| `freeclaude` | `cli/` | `install`、runtime 生命週期、設定還原、Claude 啟動與開機自啟動。 |

## Runtime

原生模式僅監聽 `127.0.0.1:3000`。Docker 模式由 `compose.yaml` 將容器的 3000 映射至相同宿主機 loopback 位址；容器以 non-root 使用者執行。兩種模式都提供相同的 API 與管理端點。

管理頁不直接操作宿主機檔案或程序，而是透過由 companion 主動建立、具 token 驗證與 allowlist 的 WebSocket RPC 執行受限操作。companion 離線時，模型 API 仍可使用，但管理動作會回報離線狀態。

## 設定與還原

敏感 token 由宿主機設定服務保護，Web Admin 僅回傳是否已設定。套用 Claude Desktop 設定時只寫入程式管理的鍵值與 metadata；`restore`／`uninstall` 只移除這些鍵值，保留使用者後續新增的設定。

## 開發與驗證

```powershell
cargo test --workspace
cargo run -p freeclaude -- install
cargo run -p freeclaude -- install --runtime docker
docker compose -f compose.yaml up --build
```
