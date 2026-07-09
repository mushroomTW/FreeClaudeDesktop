use serde_json::Value;
use std::path::Path;

pub fn compact_path(path: &Path, max_chars: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.len() <= max_chars {
        return path_str.into_owned();
    }
    let tail = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("Claude.exe");
    format!("...\\{tail}")
}

pub fn json_result(value: Value) -> Result<(), String> {
    if value.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("未知錯誤")
            .to_string())
    }
}
