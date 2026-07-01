use std::env;
use std::path::PathBuf;

/// 獲取本機的 Local AppData 目錄路徑，若環境變數不存在則使用 UserProfile 下的預設路徑。
pub fn local_app_data() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}
