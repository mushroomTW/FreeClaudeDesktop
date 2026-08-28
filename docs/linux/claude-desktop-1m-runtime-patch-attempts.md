# Claude Desktop 1M Context Runtime Patch — Retired

> **Status: RETIRED since FreeClaudeDesktop ≥ 0.1.1 / Claude Desktop ≥ 1.37x**
>
> 1M 上下文能力現已由正規 `inferenceModels[].supports1m / prefer1m` 欄位宣告，無需對 `app.asar` 進行任何 runtime 補丁。
> 本文件僅保留歷史紀錄，避免舊版使用者重複嘗試已失效的方案。

## 為何退役

* **官方已正規化**：`ClaudeSource@1.37937.1`（`.vite/build/index.chunk-CheKih7-.js`）中 `context_window` 與 `inferenceModels[].supports1m` 為官方模型中繼資料，`core/src/platform/launcher.rs:claude_config` 已正確輸出 `supports1m:true` + `prefer1m:true`。
* **完整性檢查**：`app.asar` 自 `1.36` 起加入 Electron `asar` 完整性驗證，任何二進制 patch 會導致 `Electron: Failed to load app` 並被自動還原。
* **風險**：patch 會破壞自動更新簽名，Windows Store 版（`WindowsApps\Claude_*`）路徑為唯讀，無法寫入。

## 歷史方案（僅供參考，請勿再使用）

1. **asar 解包 → 修改 `index.js` 的 `maxTokens` 上限 → 重新打包** — 在 `1.21x` 前有效，現已失效。
2. **透過 `--user-data-dir` 注入 `configLibrary/*.json` 的 `inferenceModels`** — 仍有效，即現行 `write_config_to_all_paths` 方案。
3. **Linux `LD_PRELOAD` 攔截 `open`** — 僅概念驗證，未合併。

## 自動清理

`cli/src/main.rs:install` 與 `core/src/platform/launcher.rs:ensure_mirror_profile_initialized` 會在 `mirror_profile_dir` 初始化時自動移除以下殘留（若存在）：

* `~/.config/Claude/claude_profile/.1m-patched`
* `/tmp/claude-1m-patch-*`
* `~/.local/share/FreeClaudeDesktop/patch.log`

若你仍看到 `docs/linux/*patch*` 相關日誌，請手動刪除後重新 `freecd install`。

## 正確做法（現行）

```bash
freecd dashboard
# → Model Settings → 勾選欲支援 1M 的模型 → 啟用 1M → 預設 1M
# 底層寫入：core/src/core/config.rs:model_1m_overrides + model_1m_prefer_overrides
# 轉發時：core/src/platform/launcher.rs:claude_config → inferenceModels[].supports1m/prefer1m
```

* 詳見 `docs/ARCHITECTURE.md#Model discovery and routing`。
