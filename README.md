# FreeClaudeLauncher

FreeClaudeLauncher 是跨平台桌面啟動器與本機 API proxy，用來讓 Claude Desktop 透過本機 proxy 連到使用者設定的上游 gateway。

## 功能

- 啟動本機 proxy，提供 `/v1/messages` 與 `/v1/models`。
- 支援 OpenAI-compatible 與 Anthropic-compatible API。
- 轉換 Anthropic Messages 與 OpenAI Chat Completions 的 request、response 與 streaming 格式。
- 透過 GUI 設定 gateway URL、API key、驗證方式、Claude Desktop 路徑與最佳化選項。
- 使用系統 keyring 保存 API key，Windows 另支援既有 DPAPI 格式讀取。
- 寫入 Claude Desktop `configLibrary`，並提供還原官方設定能力。
- 對 Claude Desktop 的探測、標題產生、建議模式、檔案路徑提取等請求提供本機 fast path。
- 支援 stale model route fallback，遇到上游模型下架時可改用其他已知 route 重試。

## 建置與測試

```powershell
cargo test
```

```powershell
cargo build --release
```

Windows 開發啟動：

```bat
run.bat
```

`run.bat` 只適用 Windows；Linux 與 macOS 請直接使用 Cargo。

## 目錄

```text
src/
  core/          設定、常數、錯誤型別
  platform/      跨平台路徑、憑證保護、Claude Desktop 探測與設定寫入
  runtime/       GUI 狀態與 tray 整合
  ui/            Iced UI
  server/        Axum proxy、router、models endpoint、streaming
  conversion/    Anthropic/OpenAI request 與 response 轉換
  optimization/  Claude Desktop 特殊請求 fast path
  models/        Claude 與 OpenAI/Gateway 資料模型
  lib.rs         公開 API 與設定套用流程
  main.rs        GUI 入口點
```

## 安全注意

- 不要在 log、文件、測試資料中寫入真實 API key。
- Proxy 預設只綁定 `127.0.0.1`。
- `web_fetch` 預設不允許 private network；若要開放，必須由設定明確啟用。
- 修改 proxy authorization、API key 儲存、Claude config 寫入或 request/response 轉換時，請補最小可驗證測試。

## Git

除非使用者明確同意，禁止 git commit。
