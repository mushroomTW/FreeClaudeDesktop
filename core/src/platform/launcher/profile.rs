use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use crate::common::local_app_data;
use crate::error::AppResult;
use crate::platform::atomic_file::{PendingWrite, write_transaction};

use super::{app_data_roaming_dir, apply_3p_deployment_mode, update_config_port};

/// 回傳 Desktop 標準第三方資料目錄；保留原函式名稱供既有呼叫者使用。
pub fn mirror_profile_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        local_app_data().join("Claude-3p")
    }
    #[cfg(not(target_os = "windows"))]
    {
        app_data_roaming_dir().join("Claude-3p")
    }
}

/// 回傳官方 Claude 使用者資料目錄。
pub fn official_app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // 新版 Store 可能已將第一方資料搬出套件沙箱。
        let relocated = local_app_data().join("Claude-Data");
        for candidate in [
            relocated.join("claude_pzs8sxrjxfjjc"),
            relocated.join("anthropicpbc.claude_fnn82j28hfe8t"),
            relocated,
        ] {
            if candidate.join("ant-did").is_file() {
                return candidate;
            }
        }
        let roaming = app_data_roaming_dir().join("Claude");
        if roaming.exists() {
            return roaming;
        }
        for package in ["Claude_pzs8sxrjxfjjc", "AnthropicPBC.Claude_fnn82j28hfe8t"] {
            let candidate = local_app_data()
                .join("Packages")
                .join(package)
                .join("LocalCache")
                .join("Roaming")
                .join("Claude");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    app_data_roaming_dir().join("Claude")
}

/// 遞迴複製目錄內容；來源不存在時不做任何處理。
pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    let src = src.as_ref();
    let dst = dst.as_ref();

    if !src.exists() {
        return Ok(());
    }

    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir_all(source, target)?;
        } else if file_type.is_file() {
            fs::copy(source, target)?;
        }
    }
    Ok(())
}

/// 同步一般資料，但第三方設定庫由本專案管理，不能被第一方資料覆寫。
fn copy_official_profile(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name() == "configLibrary" {
            continue;
        }
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(entry.path(), target)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// 第三方資料不存在時，從官方資料初始化。
pub fn ensure_mirror_profile_initialized() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    if mirror.exists() {
        return Ok(());
    }

    let official = official_app_data_dir();
    if official.exists() {
        copy_official_profile(&official, &mirror)?;
    } else {
        fs::create_dir_all(mirror)?;
    }
    Ok(())
}

/// 以官方資料重新同步鏡像，並重新套用目前的部署設定。
pub fn resync_from_official() -> AppResult<()> {
    let official = official_app_data_dir();
    let mirror = mirror_profile_dir();
    if official.exists() {
        copy_official_profile(&official, &mirror)?;
    }

    let port = crate::config::load_launcher_settings()?
        .and_then(|settings| settings.desktop.active_port)
        .unwrap_or(crate::constants::DEFAULT_PORT);
    apply_3p_deployment_mode()?;
    update_config_port(port)
}

/// 重建鏡像資料；初始化失敗時還原原目錄。
pub fn reset_mirror_profile() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    // 重建瀏覽器資料時保留目前 Proxy 設定與設定庫選擇。
    let mut managed = Vec::new();
    for name in [
        format!("{}.json", crate::constants::CONFIG_ID),
        "_meta.json".to_string(),
    ] {
        let path = mirror.join("configLibrary").join(name);
        if path.exists() {
            let contents = fs::read(&path)?;
            managed.push(PendingWrite::new(path, contents));
        }
    }
    let backup = mirror.with_file_name(format!(
        "{}.reset-backup-{}",
        mirror
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Claude-3p"),
        std::process::id()
    ));

    if mirror.exists() {
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        fs::rename(&mirror, &backup)?;
    }

    let result = (|| {
        ensure_mirror_profile_initialized()?;
        fs::create_dir_all(mirror.join("configLibrary"))?;
        write_transaction(managed)?;
        apply_3p_deployment_mode()
    })();
    if result.is_err() && backup.exists() {
        let _ = fs::remove_dir_all(&mirror);
        let _ = fs::rename(&backup, &mirror);
    } else if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    result
}
