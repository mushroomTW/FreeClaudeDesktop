use std::fs;
use std::path::{Path, PathBuf};

use crate::common::local_app_data;
use crate::error::AppResult;

use super::{app_data_roaming_dir, apply_3p_deployment_mode, update_config_port};

/// 回傳鏡像 Claude 使用者資料目錄。
pub fn mirror_profile_dir() -> PathBuf {
    local_app_data()
        .join("FreeClaudeDesktop")
        .join("claude_profile")
}

/// 回傳官方 Claude 使用者資料目錄。
pub fn official_app_data_dir() -> PathBuf {
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

/// 在鏡像資料不存在時，從舊版目錄或官方資料初始化。
pub fn ensure_mirror_profile_initialized() -> AppResult<()> {
    let mirror = mirror_profile_dir();
    if mirror.exists() {
        return Ok(());
    }

    let old_profile = local_app_data().join("Claude-3p");
    if old_profile.exists() {
        copy_dir_all(old_profile, mirror)?;
    } else {
        let official = official_app_data_dir();
        if official.exists() {
            copy_dir_all(official, mirror)?;
        } else {
            fs::create_dir_all(mirror)?;
        }
    }
    Ok(())
}

/// 以官方資料重新同步鏡像，並重新套用目前的部署設定。
pub fn resync_from_official() -> AppResult<()> {
    let official = official_app_data_dir();
    let mirror = mirror_profile_dir();
    if official.exists() {
        copy_dir_all(official, mirror)?;
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
    let backup = mirror.with_file_name(format!(
        "{}.reset-backup-{}",
        mirror
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("claude_profile"),
        std::process::id()
    ));

    if mirror.exists() {
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        fs::rename(&mirror, &backup)?;
    }

    let result = ensure_mirror_profile_initialized();
    if result.is_err() && backup.exists() {
        let _ = fs::remove_dir_all(&mirror);
        let _ = fs::rename(&backup, &mirror);
    } else if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    result
}
