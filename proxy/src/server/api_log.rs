use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const API_LOG_FILE: &str = "api-calls.log";
const MAX_API_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// 活動檔加上四個封存檔，最多保留五個 10 MiB 檔案。
const MAX_API_LOG_ARCHIVES: usize = 4;

static API_LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static API_CALL_SEQUENCE: OnceLock<std::sync::atomic::AtomicU64> = OnceLock::new();

/// 取得可關聯單一 API 呼叫的遞增識別碼。
pub fn next_call_id() -> u64 {
    use std::sync::atomic::Ordering;

    API_CALL_SEQUENCE
        .get_or_init(|| std::sync::atomic::AtomicU64::new(1))
        .fetch_add(1, Ordering::Relaxed)
}

/// 將不含敏感內容的 API 呼叫摘要寫入受限大小的 JSON Lines 紀錄檔。
pub fn record_api_call(record: Value) {
    let log_dir = free_claude_core::common::local_app_data()
        .join("FreeClaudeDesktop")
        .join("logs");
    let lock = API_LOG_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = lock.lock() else {
        return;
    };

    if let Err(error) = append_record(&log_dir, &record, MAX_API_LOG_BYTES, MAX_API_LOG_ARCHIVES) {
        tracing::warn!("無法寫入 API 呼叫紀錄：{error}");
    }
}

/// 產生供 API 紀錄使用的 Unix 時間戳記（毫秒）。
pub fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn append_record(
    log_dir: &Path,
    record: &Value,
    max_bytes: u64,
    max_files: usize,
) -> std::io::Result<()> {
    fs::create_dir_all(log_dir)?;
    let mut line =
        serde_json::to_vec(record).map_err(|error| std::io::Error::other(error.to_string()))?;
    line.push(b'\n');

    let active_path = log_dir.join(API_LOG_FILE);
    let active_size = fs::metadata(&active_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    if active_size.saturating_add(line.len() as u64) > max_bytes {
        rotate_logs(log_dir, max_files)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_path)?;
    file.write_all(&line)
}

fn rotate_logs(log_dir: &Path, max_files: usize) -> std::io::Result<()> {
    if max_files == 0 {
        return Ok(());
    }

    let archive_path = |index: usize| -> PathBuf { log_dir.join(format!("api-calls.{index}.log")) };
    let oldest = archive_path(max_files);
    if oldest.exists() {
        fs::remove_file(oldest)?;
    }
    for index in (1..max_files).rev() {
        let source = archive_path(index);
        if source.exists() {
            fs::rename(source, archive_path(index + 1))?;
        }
    }

    let active = log_dir.join(API_LOG_FILE);
    if active.exists() {
        fs::rename(active, archive_path(1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_call_log_rotates_and_limits_archive_count() {
        let path = std::env::temp_dir().join(format!(
            "freeclaudedesktop-api-log-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);

        for index in 0..4 {
            append_record(
                &path,
                &json!({ "index": index, "padding": "x".repeat(100) }),
                128,
                2,
            )
            .unwrap();
        }

        assert!(path.join(API_LOG_FILE).exists());
        assert!(path.join("api-calls.1.log").exists());
        assert!(!path.join("api-calls.3.log").exists());
        let _ = fs::remove_dir_all(path);
    }
}
