# AGENTS.md

## 語言與溝通

- 請始終使用繁體中文與使用者溝通。
- 程式碼註解與文件說明亦請優先使用繁體中文。
- 新增或修改文字檔時維持 UTF-8 編碼。
- 若遇到既有亂碼，先確認是否與本次任務相關；非必要不要順手重寫無關檔案。

## 項目目標

- 提供支援 Windows、Linux、macOS 的跨平台桌面啟動器，協助使用者設定並啟動 Claude Desktop。
- 在本機啟動 API proxy，將 Claude Desktop 的 Anthropic Messages 請求轉接到使用者設定的上游 gateway。
- 支援 OpenAI-compatible 與 Anthropic-compatible API，並處理 request、response、streaming 格式轉換。
- 安全保存 API key，優先使用跨平台方案；平台特定儲存邏輯必須隔離。
- 管理本專案寫入的 Claude Desktop 設定項，並保留可還原能力。
- 對 Claude Desktop 的探測、標題產生、建議模式、檔案路徑提取等特殊請求做最小必要最佳化，減少無效上游成本。
- 遇到上游回報模型下架時，支援 stale model route fallback 與一次重試。
- 保持與 Claude Desktop 實際行為相容，不用猜測代替驗證。

## Claude Desktop 邊界

- 不得修改 Claude Desktop 的原始碼、安裝檔或內建資源。
- 只能修改本專案的 launcher、proxy、config 寫入邏輯與相容層。
- 修改 configLibrary、Anthropic Messages API 相容、模型 alias、tool use、streaming、thinking、探測請求等相關功能前，必須先查明 Claude Desktop 端實際期待格式。
- 請參考 Claude Desktop 原始碼:"C:\Users\mushroomMaster\Documents\ClaudeSource"，先閱讀相依區塊再修改本專案。

## 跨平台優先級

- 代碼以同時兼容 Windows、Linux、macOS 為優先。
- 預設寫可攜 Rust；只有平台 API 必須分流時才使用 `#[cfg(...)]`。
- 平台差異應集中在小函式或小模組，避免 Windows-only 假設散落在 business logic。
- 新功能需先確認三個平台的路徑、系統呼叫、托盤、憑證儲存、Claude config 位置與 GUI 行為。
- Windows-only 命令或腳本要標明限制；不要把 `run.bat` 當成跨平台入口。

## 專案概覽

- FreeClaudeLauncher 是跨平台桌面啟動器與本機 API proxy。
- 主要二進位名稱為 `FreeClaudeLauncher`，使用 Rust 2021。
- Launcher 負責 GUI 設定、Claude Desktop 探測與啟動、configLibrary 寫入與還原。
- Proxy 提供 `/v1/messages` 與 `/v1/models`，轉接 Claude Desktop 到使用者設定的上游 gateway。
- 支援 OpenAI-compatible 與 Anthropic-compatible API，包含 request、response、streaming 轉換。
- 內建 Claude Desktop 特殊請求 fast path 與 stale model route fallback。
- GUI 使用 `iced`，系統托盤使用 `tray-icon`。
- 本機 HTTP proxy 使用 `axum`、`tokio`、`reqwest`。
- 設定與序列化使用 `serde`、`serde_json`。
- API key 儲存使用 `keyring`，Windows 另有 DPAPI 相容邏輯。

## 常用命令

- `cargo test`：執行 Rust 測試。
- `cargo build --release`：建立 release 版本。
- `run.bat`：Windows 開發啟動腳本；Linux/macOS 不適用。

## 專案結構

- `src/core/`：launcher settings、常數、錯誤型別。
- `src/platform/`：跨平台路徑、API key 保護、Claude Desktop 探測、啟動、config 寫入與還原。
- `src/runtime/`：GUI 狀態、事件更新邏輯與 tray 整合。
- `src/ui/`：Iced UI view 與樣式。
- `src/server/`：本機 proxy、router、handler、models endpoint、streaming。
- `src/conversion/`：Anthropic 與 OpenAI-compatible request/response 轉換、model route rewrite。
- `src/optimization/`：Claude Desktop 特殊請求的本機 fast path 與安全邊界。
- `src/models/`：Claude/OpenAI 資料模型。
- `src/lib.rs`：公開 API、設定套用流程與向後相容 re-export。
- `src/main.rs`：GUI 入口點。

## 修改原則

- 小改、少檔案、優先重用現有 helper。
- 不新增不必要 abstraction、factory、trait 或 dependency。
- 非必要不要重排大型檔案或格式化無關區塊。
- 涉及請求轉換時，優先補在共用轉換函式，不要只修單一路徑。
- 修改設定格式時要保留向後相容，避免破壞既有 `launcher_settings.json`。

## 測試要求

- 轉換邏輯變更需跑或補 `src/conversion/` 與 `src/lib.rs` 內相關測試。
- streaming 變更需跑或補 `src/server/streaming.rs` 相關測試。
- authorization、settings migration、optimization、平台分流、Claude Desktop 相容性變更需補最小可驗證測試。
- 文件-only 變更不必跑完整測試，但需檢查內容是否與專案現況一致。

## 安全與資料保護

- API key、keyring/DPAPI、Claude config、proxy authorization、private network web fetch 屬於高風險區。
- 不要在 log、錯誤訊息、測試 fixture 或文件中寫入真實 API key。
- 不要放寬 proxy authorization，除非同時說明威脅模型並補測試。
- web fetch 預設不得擴大到 private network；若需支援，必須受設定控制。

## UI 注意事項

- UI 需維持跨平台可用，不寫死 Windows 字型、路徑或視窗假設。
- Iced 元件變更應保持現有密度與簡潔風格。
- 新增文字需能在固定視窗寬度內顯示，不要讓按鈕或狀態列溢出。

## Git 規則

- 除非使用者同意，否則禁止 git commit。
- commit message裡要包含所有git diff概述。
- 不得覆蓋、重置或移除使用者既有未提交變更。
- 若工作區已有無關變更，只處理本次任務需要的檔案。
