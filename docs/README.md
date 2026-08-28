# 文件索引

本目錄集中專案的介紹與說明文件，`README.md` 仍保留於專案根目錄作為入口。

## 目錄

- [架構說明](ARCHITECTURE.md) — Crate 邊界、Runtime 拓撲、訊息執行流程、模型探索與狀態持有
- [擴充與本地技能](EXTENSIONS_AND_SKILLS.md) — 本地優化與工具攔截機制的詳細介紹（繁體中文）

## 相關入口

- 根目錄 [README.md](../README.md)（English）與 [README_zh.md](../README_zh.md)（繁體中文）— 快速開始、安裝、設定、CLI 參考、Proxy API
- `sonar-project.properties`（根目錄）— SonarQube 分析設定

> **維護約定**：所有對外介紹性 Markdown（架構、功能說明）請置於 `docs/` 下，根目錄僅保留 `README.md` / `README_zh.md` / `LICENSE` 與配置檔，避免根目錄膨脹。
