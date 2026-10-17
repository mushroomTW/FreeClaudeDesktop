# 文件索引

本目錄集中專案的介紹與說明文件，`README.md` 仍保留於專案根目錄作為入口。

## 目錄

- [架構說明](ARCHITECTURE.md) — Crate 邊界、Runtime 拓撲、訊息執行流程、模型探索與狀態持有
- [擴充與本地技能](EXTENSIONS_AND_SKILLS.md) — 本地優化與工具攔截機制的詳細介紹（繁體中文）

## 相關入口

- 根目錄 [README.md](../README.md)（English）與 [README_zh.md](../README_zh.md)（繁體中文）— 快速開始、安裝、設定、CLI 參考、Proxy API
- `sonar-project.properties`（根目錄）— SonarQube 分析設定

> **維護約定**：所有對外介紹性 Markdown（架構、功能說明）請置於 `docs/` 下，根目錄僅保留 `README.md` / `README_zh.md` / `LICENSE` 與配置檔，避免根目錄膨脹。

## 語言約定

- CLI 使用者可見文字一律使用英文：終端輸出、`--help` 的 about 文字、錯誤訊息、日誌，以及 `core/`、`proxy/` 冒泡給使用者的訊息與 API 錯誤本文。
- 使用者可見的標籤常數（例如 `core::constants::PROVIDERS`）同採英文。
- Web Dashboard 的語系表（`core/src/core/config.rs` 的 `En`/`ZhTw` 對照、`proxy/src/server/dashboard.js` 的 `translations`）是刻意的多語系資源，維持原文。Dashboard 顯示伺服器錯誤時，以回應中的 `code` 對照語系表；沒有 `code` 時才顯示伺服器訊息（英文診斷）。
- 程式碼註解與 `docs/` 文件預設維持繁體中文。
- 適用範圍為受版控的專案程式碼；`.scratch/` 的本機暫存腳本與舊版封裝產物（例如 `.scratch/live-bun/package/`）不在此限。
- 本節是版本控管的正式來源；根目錄 `AGENTS.md` 是同一份約定的本機副本（已被 `.gitignore` 排除）。
