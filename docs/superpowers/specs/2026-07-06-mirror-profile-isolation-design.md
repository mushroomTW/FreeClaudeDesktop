# FreeClaudeLauncher 鏡像目錄與數據隔離架構設計

## 1. 概述

本設計旨在改變 FreeClaudeLauncher 對 Claude Desktop 配置檔與 AppData 目錄的操作模式。
原本的運作模式會直接修改 `%APPDATA%\Claude` 及 `%APPDATA%\Claude-3p` 內的 `claude_desktop_config.json` 與 `settings.json`；新的運作模式將採用**獨立 Profile 鏡像目錄 (Isolated Mirror Profile Directory)** 方案，透過 Electron 原生的 `--user-data-dir` 參數隔離所有配置變更。

---

## 2. 核心原則

1. **原版數據 100% 唯讀保護**：
   原版 Claude Desktop 的資料目錄（`%APPDATA%\Claude` 或 macOS/Linux 對應路徑）僅在同步時做**唯讀讀取**，FreeClaudeLauncher 絕不寫入、修改或刪除原版目錄下的任何檔案。
2. **獨立 Profile 鏡像目錄**：
   所有 FreeClaudeLauncher 產生的配置變更（包含 `deploymentMode: "3p"`、`configLibrary/free_claude_launcher.json`、`ANTHROPIC_BASE_URL` 與本機 Computer MCP 伺服器等）均只存放在專屬鏡像目錄 `%LOCALAPPDATA%\FreeClaudeLauncher\claude_profile`。
3. **無縫切換與獨立運行**：
   當使用者不透過 FreeClaudeLauncher 啟動、直接開啟原版 Claude Desktop 時，系統會直接加載未被任何修改的官方原生狀態，無需執行複雜的還原作業。

---

## 3. 跨平台架構與流程設計

### 3.1 跨平台鏡像與原版目錄對照

| 作業系統 | 原版資料目錄 (Original Official Path) | 鏡像 Profile 目錄 (Isolated Mirror Profile Path) |
| :--- | :--- | :--- |
| **Windows** | `%APPDATA%\Claude` (Roaming) / `%LOCALAPPDATA%\Claude-3p` | `%LOCALAPPDATA%\FreeClaudeLauncher\claude_profile` |
| **macOS** | `~/Library/Application Support/Claude` | `~/Library/Application Support/FreeClaudeLauncher/claude_profile` |
| **Linux** | `~/.config/Claude` (或 `$XDG_CONFIG_HOME/Claude`) | `~/.config/FreeClaudeLauncher/claude_profile` (或 `$XDG_CONFIG_HOME/FreeClaudeLauncher/claude_profile`) |

### 3.2 跨平台啟動流程 (`launch_claude`)
當使用者透過 FreeClaudeLauncher 啟動 Claude Desktop 時：
1. 自動探測並確保當前平台的鏡像目錄是否存在；若不存在，自動執行跨平台「首次同步 (First-time Sync)」。
2. 在鏡像目錄內套用/更新 FreeClaudeLauncher 代理配置（寫入 `configLibrary/free_claude_launcher.json` 與 `claude_desktop_config.json`）。
3. 根據作業系統啟動 Claude 執行檔並帶入原生 Electron 隔離參數：
   - **Windows / Linux**: `Command::new(claude_bin).arg(format!("--user-data-dir={}", mirror_dir.display()))`
   - **macOS**: `Command::new(claude_bin).arg(format!("--user-data-dir={}", mirror_dir.display()))` (或 `open -n -a Claude.app --args --user-data-dir=...`)

### 3.3 資料同步機制

#### 1. 首次啟動同步 (First-Time Sync)
- 當鏡像目錄尚未建立時，讀取當前平台原版 AppData 內的關鍵登入與設定資料：
  - `claude_desktop_config.json` (原版自訂 MCP 伺服器)
  - `Local Storage/` / `IndexedDB/` / `storage/` (登入 Session 與 UI 偏好)
- 在鏡像目錄內寫入代理 3P 模式與 MCP 設定。

#### 2. 手動從原版重新同步 (Re-sync from Official)
- 當使用者在原版 Claude Desktop 登入新帳號或新增 MCP 後，可點擊 UI 上的「從原版重新同步」按鈕。
- 程式將複製原版最新設定與 Session 至鏡像目錄，並重新套用 FreeClaudeLauncher 的 3P 代理與 MCP 配置。

#### 3. 重置鏡像目錄 (Reset Mirror Profile)
- 當使用者點擊「重置鏡像目錄」時，僅刪除鏡像目錄內容，不會對官方原版目錄有任何影響。

---

## 4. UI 與控制邏輯調整

1. UI 底部按鈕區更新：
   - 新增 **「從原版重新同步」** 按鈕。
   - 將原本的 **「還原官方設定」** 標題與說明更新為 **「重置鏡像目錄」**，並提示原版目錄完全不受影響。
2. 訊息列與提示說明優化：
   - 明確顯示當前鏡像目錄路徑與同步狀態。

---

## 5. 驗證計畫

1. **自動測試**：
   - 單元測試鏡像目錄路徑計算、首次複製與過濾邏輯。
   - 單元測試 MCP 合併與代理寫入皆在鏡像目錄內進行。
2. **手動測試**：
   - 驗證啟動 Claude Desktop 時附帶 `--user-data-dir` 參數。
   - 驗證原版 AppData 內容完全未被修改。
   - 驗證點擊「從原版重新同步」與「重置鏡像目錄」功能正確運作。
