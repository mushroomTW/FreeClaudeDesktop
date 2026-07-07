# 鏡像 Profile 目錄隔離實作計畫 (Mirror Profile Isolation Implementation Plan)

> **代理任務指示：** 必要子技能：請使用 superpowers:subagent-driven-development (推薦) 或 superpowers:executing-plans 來逐項執行本計畫。步驟使用複選框 (`- [ ]`) 語法追蹤。

**目標：** 實作獨立的鏡像 Profile 目錄 (`--user-data-dir`)，使 FreeClaudeLauncher 在專屬目錄下運作，支援 Windows、macOS 與 Linux，並且完全不修改官方原版的 Claude Desktop 檔案。

**架構概述：** 新增 `mirror_profile_dir()` 工具函式。當啟動 Claude Desktop 時，傳遞 `--user-data-dir=<鏡像目錄>`。當鏡像目錄為空時自動執行首次複製同步。將所有配置變更（3P 代理、configLibrary、MCP 伺服器）改為寫入鏡像 Profile 目錄中。更新介面按鈕以支援「從原版重新同步」與「重置鏡像目錄」。

**技術棧：** Rust (2021 edition), `std::fs`, `std::env`, `serde_json`, `iced` UI 框架。

## 全域約束

- **語言偏好：** UI 文字、程式碼註解與文件說明必須全程使用繁體中文。
- **Git 約束：** 除非使用者明確要求，否則禁止執行 `git commit`。
- **跨平台相容性：** 必須使用 Rust 標準庫與平台分流邏輯，完整兼容 Windows、macOS 與 Linux。

---

### 任務 1：新增跨平台鏡像 Profile 與原版目錄路徑工具函式

**檔案範圍：**
- 修改：`src/platform/launcher.rs`
- 修改：`src/platform/common.rs`
- 測試：`src/platform/launcher.rs`

**介面定義：**
- 提供：`mirror_profile_dir() -> PathBuf`, `official_app_data_dir() -> PathBuf`

- [ ] **步驟 1：撰寫失敗的單元測試**

在 `src/platform/launcher.rs` 新增測試：
```rust
#[test]
fn mirror_profile_dir_returns_valid_path() {
    let mirror = mirror_profile_dir();
    assert!(mirror.to_string_lossy().contains("FreeClaudeLauncher"));
    assert!(mirror.to_string_lossy().contains("claude_profile"));
}

#[test]
fn official_app_data_dir_returns_valid_path() {
    let official = official_app_data_dir();
    assert!(official.to_string_lossy().contains("Claude"));
}
```

- [ ] **步驟 2：執行測試確認失敗**

執行：`cargo test platform::launcher::tests::mirror_profile_dir_returns_valid_path`
預期：失敗 (提示找不到 `mirror_profile_dir` 函式)

- [ ] **步驟 3：實作 `mirror_profile_dir` 與 `official_app_data_dir`**

在 `src/platform/launcher.rs` 實作：
```rust
pub fn mirror_profile_dir() -> PathBuf {
    local_app_data().join("FreeClaudeLauncher").join("claude_profile")
}

pub fn official_app_data_dir() -> PathBuf {
    app_data_roaming_dir().join("Claude")
}
```

- [ ] **步驟 4：執行測試確認通過**

執行：`cargo test platform::launcher::tests::mirror_profile_dir_returns_valid_path`
預期：通過 (PASS)

---

### 任務 2：實作鏡像目錄的複製同步與重置工具函式

**檔案範圍：**
- 修改：`src/platform/launcher.rs`
- 測試：`src/platform/launcher.rs`

**介面定義：**
- 使用：`mirror_profile_dir()`, `official_app_data_dir()`
- 提供：`ensure_mirror_profile_initialized() -> AppResult<()>`, `resync_from_official() -> AppResult<()>`, `reset_mirror_profile() -> AppResult<()>`

- [ ] **步驟 1：撰寫複製與同步邏輯的測試**

```rust
#[test]
fn sync_copies_official_files_to_temp_mirror() {
    let temp_dir = std::env::temp_dir().join(format!("fcl_test_mirror_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    
    let result = initialize_mirror_from_source(&official_app_data_dir(), &temp_dir);
    assert!(result.is_ok());
    assert!(temp_dir.exists());
    let _ = fs::remove_dir_all(&temp_dir);
}
```

- [ ] **步驟 2：實作遞迴目錄複製與同步函式**

在 `src/platform/launcher.rs` 中實作遞迴複製 `copy_dir_all`，以及 `ensure_mirror_profile_initialized`、`resync_from_official` 與 `reset_mirror_profile`。

- [ ] **步驟 3：執行測試確認通過**

執行：`cargo test`
預期：通過 (PASS)

---

### 任務 3：更新 `launch_claude` 帶入 `--user-data-dir` 參數

**檔案範圍：**
- 修改：`src/platform/launcher.rs`

**介面定義：**
- 使用：`mirror_profile_dir()`, `ensure_mirror_profile_initialized()`
- 修改：`launch_claude`

- [ ] **步驟 1：更新各平台的 `launch_claude` 邏輯**

在 `src/platform/launcher.rs` 中：
啟動 Claude 執行檔前，先確保鏡像目錄已初始化 (`ensure_mirror_profile_initialized()?`)。
將 `--user-data-dir=<鏡像目錄>` 加入啟動參數：
```rust
Command::new(&target)
    .arg(format!("--user-data-dir={}", mirror_profile_dir().display()))
    .spawn()
```

- [ ] **步驟 2：執行測試確認通過**

執行：`cargo test`
預期：通過 (PASS)

---

### 任務 4：將配置讀寫重定向至鏡像目錄

**檔案範圍：**
- 修改：`src/platform/launcher.rs`

**介面定義：**
- 修改：`mcp_config_paths()`, `config_library_dirs()`

- [ ] **步驟 1：將 `mcp_config_paths()` 與 `config_library_dirs()` 目標指向鏡像目錄**

在 `src/platform/launcher.rs` 中：
`mcp_config_paths()` 回傳 `vec![mirror_profile_dir().join("claude_desktop_config.json")]`。
`config_library_dirs()` 回傳 `vec![mirror_profile_dir().join("configLibrary")]`。

- [ ] **步驟 2：執行測試確認通過**

執行：`cargo test`
預期：通過 (PASS)

---

### 任務 5：新增 UI 按鈕「從原版重新同步」與「重置鏡像目錄」

**檔案範圍：**
- 修改：`src/runtime/app.rs`
- 修改：`src/ui/view.rs`

**介面定義：**
- 新增：`Message::ResyncFromOfficial`, `Message::ResetMirrorProfile`

- [ ] **步驟 1：在 `app.rs` 中新增與處理新的 Message**

在 `src/runtime/app.rs` 中：
為 `Message` 枚舉新增 `ResyncFromOfficial` 與 `ResetMirrorProfile`。
在 `update()` 中分別呼叫 `launcher::resync_from_official()` 與 `launcher::reset_mirror_profile()`。

- [ ] **步驟 2：在 `view.rs` 中新增與調整 UI 按鈕**

在 `src/ui/view.rs` 中：
新增「從原版重新同步」按鈕，綁定 `Message::ResyncFromOfficial`。
將原還原按鈕更名為「重置鏡像目錄」，綁定 `Message::ResetMirrorProfile`。

- [ ] **步驟 3：執行測試確認通過**

執行：`cargo test`
預期：通過 (PASS)
