//! Request detection utilities for local optimizations.
//!
//! Detects quota checks, prefix commands, title generation, safety classifier,
//! suggestion mode, and filepath extraction requests.

use crate::optimization::command_utils::{parse_shell_command_prefix, strip_env_assignments};
use serde_json::Value;

/// 從 JSON body 的 `system` 欄位提取文字，支援 String 和 Array 格式。
///
/// Anthropic Messages API 允許 `system` 為以下兩種格式：
/// - `"system": "some text"` （String）
/// - `"system": [{"type": "text", "text": "some text"}, ...]` （Array）
///
/// Claude Code 傾向使用 Array 格式，原先的偵測邏輯只處理 String，
/// 導致所有依賴 system 的偵測（標題生成、安全分類器等）全部失敗。
fn extract_system_text(v: &Value) -> Option<String> {
    match v.get("system")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(arr) => {
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Check if this is a quota probe request.
///
/// Quota checks are typically simple requests with max_tokens=1
/// and a single message containing the word "quota".
pub fn is_quota_check_request(body_str: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };

    let max_tokens = v.get("max_tokens").and_then(Value::as_u64);
    if max_tokens != Some(1) {
        return false;
    }

    let Some(messages) = v.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.len() != 1 {
        return false;
    }

    let content = messages
        .first()
        .and_then(|message| message.get("content"))
        .map(|content| match content {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .unwrap_or_default();
    let trimmed = content.trim();
    let lower = trimmed.to_lowercase();

    let is_user = messages[0].get("role").and_then(Value::as_str) == Some("user");
    let has_tools = v
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    is_user && has_tools && lower == "count"
}

/// Check if this is a command prefix detection request.
///
/// Returns the detected prefix if matched.
pub fn extract_command_prefix(body_str: &str) -> Option<String> {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return None;
    };

    let messages = v.get("messages").and_then(Value::as_array)?;
    if messages.len() != 1 {
        return None;
    }

    let content = messages
        .first()?
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    if !content.contains("<policy_spec>") || !content.contains("Command:") {
        return None;
    }

    let cmd_part = content.split("Command:").nth(1)?.trim();
    let prefix = parse_shell_command_prefix(cmd_part);
    Some(prefix)
}

/// Check if this is a conversation title generation request.
///
/// Detected by a system prompt containing title extraction instructions,
/// no tools, and a single user message.
pub fn is_title_generation_request(body_str: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };

    let has_system = v.get("system").is_some();
    let has_tools = v.get("tools").is_some();
    if !has_system || has_tools {
        return false;
    }

    let system_text = match extract_system_text(&v) {
        Some(s) => s.to_lowercase(),
        None => return false,
    };

    if !system_text.contains("title") {
        return false;
    }

    system_text.contains("sentence-case title")
        || system_text.contains("<title>")
        || (system_text.contains("title")
            && (system_text.contains("generate a short title")
                || system_text.contains("concise title")
                || system_text.contains("suggest a title")
                || (system_text.contains("return json")
                    && system_text.contains("field")
                    && (system_text.contains("coding session")
                        || system_text.contains("this session")))))
}

/// Check if this is a suggestion mode request.
///
/// Suggestion mode requests contain "[SUGGESTION MODE:" in the user's message.
pub fn is_suggestion_mode_request(body_str: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };

    let messages = v.get("messages").and_then(Value::as_array);
    if messages.is_none() {
        return false;
    }

    messages
        .unwrap()
        .iter()
        .filter_map(|msg| {
            if msg.get("role")? == "user" {
                msg.get("content").and_then(|c| c.as_str())
            } else {
                None
            }
        })
        .any(|text| text.contains("[SUGGESTION MODE:"))
}

/// Check if this is a filepath extraction request.
///
/// Filepath extraction requests have a single user message with
/// "Command:" and "Output:" sections.
///
/// Returns the extracted filepaths string if matched.
pub fn extract_filepaths(body_str: &str) -> Option<String> {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return None;
    };

    let messages = v.get("messages").and_then(Value::as_array)?;
    if messages.len() != 1 {
        return None;
    }

    if v.get("tools").is_some() {
        return None;
    }

    let content = messages
        .first()?
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    if !content.contains("Command:") || !content.contains("Output:") {
        return None;
    }

    let system_text = extract_system_text(&v).unwrap_or_default().to_lowercase();

    let user_has_filepaths = content.to_lowercase().contains("filepaths");
    let system_has_extract = system_text.contains("extract any file paths")
        || system_text.contains("file paths that this command");

    if !user_has_filepaths && !system_has_extract {
        return None;
    }

    let cmd_start = content.find("Command:")? + "Command:".len();
    let output_marker = content.find("Output:")?;
    if output_marker <= cmd_start {
        return None;
    }

    let command = content[cmd_start..output_marker].trim();
    let output = &content[output_marker + "Output:".len()..];
    let output = output.split(['<', '\n']).next().unwrap_or(output).trim();

    let filepaths = extract_filepaths_from_command(command, output);
    Some(filepaths)
}

/// Extract filepaths from a command and its output locally.
///
/// Determines if the command reads file contents and extracts paths.
fn extract_filepaths_from_command(command: &str, _output: &str) -> String {
    let listing_cmds = [
        "ls", "dir", "find", "tree", "pwd", "cd", "mkdir", "rmdir", "rm",
    ];
    let reading_cmds = ["cat", "head", "tail", "less", "more", "bat", "type"];

    let parts: Vec<&str> = command.split_whitespace().collect();
    let parts = strip_env_assignments(&parts);
    if parts.is_empty() {
        return "<filepaths>\n</filepaths>".to_string();
    }

    let base_cmd = parts[0]
        .rsplit('/')
        .next()
        .unwrap_or(parts[0])
        .rsplit('\\')
        .next()
        .unwrap_or(parts[0])
        .to_lowercase();

    if listing_cmds.contains(&base_cmd.as_str()) {
        return "<filepaths>\n</filepaths>".to_string();
    }

    if reading_cmds.contains(&base_cmd.as_str()) {
        let paths: Vec<String> = parts[1..]
            .iter()
            .filter(|p| !p.starts_with('-'))
            .map(|p| p.to_string())
            .collect();

        if paths.is_empty() {
            return "<filepaths>\n</filepaths>".to_string();
        }
        return format!("<filepaths>\n{}\n</filepaths>", paths.join("\n"));
    }

    if base_cmd == "grep" {
        let flags_with_args = ["-e", "-f", "-m", "-A", "-B", "-C"];
        let mut skip_next = false;
        let mut positional: Vec<&str> = Vec::new();
        let mut pattern_provided = false;

        for part in &parts[1..] {
            if skip_next {
                skip_next = false;
                continue;
            }
            if part.starts_with('-') {
                if flags_with_args.contains(&&**part) {
                    if *part == "-e" || *part == "-f" {
                        pattern_provided = true;
                    }
                    skip_next = true;
                }
                continue;
            }
            positional.push(*part);
        }

        let filepaths = if pattern_provided {
            positional
        } else {
            positional.into_iter().skip(1).collect::<Vec<_>>()
        };

        if filepaths.is_empty() {
            return "<filepaths>\n</filepaths>".to_string();
        }
        return format!("<filepaths>\n{}\n</filepaths>", filepaths.join("\n"));
    }

    "<filepaths>\n</filepaths>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_system_text_string_and_array() {
        let string_system = json!({ "system": "Hello world" });
        assert_eq!(extract_system_text(&string_system).unwrap(), "Hello world");

        let array_system = json!({
            "system": [
                { "type": "text", "text": "Part 1" },
                { "type": "text", "text": "Part 2" }
            ]
        });
        assert_eq!(
            extract_system_text(&array_system).unwrap(),
            "Part 1\nPart 2"
        );
    }

    #[test]
    fn test_is_title_generation_request_array_system() {
        let body = json!({
            "system": [
                { "type": "text", "text": "Generate a sentence-case title for this chat" }
            ],
            "messages": [{ "role": "user", "content": "hello" }]
        })
        .to_string();
        assert!(is_title_generation_request(&body));
    }

    #[test]
    fn test_is_quota_check_request_token_count_and_env() {
        let count_body = json!({
            "max_tokens": 1,
            "tools": [{"name": "test_tool"}],
            "messages": [{ "role": "user", "content": "count" }]
        })
        .to_string();
        assert!(is_quota_check_request(&count_body));

        let env_body = json!({
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "# Environment\nYou are running in Windows..." }]
        })
        .to_string();
        assert!(!is_quota_check_request(&env_body));

        let normal_body = json!({
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "Answer yes or no." }]
        })
        .to_string();
        assert!(!is_quota_check_request(&normal_body));
    }

    #[test]
    fn ordinary_short_quota_question_is_not_a_probe() {
        let body = json!({
            "max_tokens": 1,
            "messages": [{"role":"user","content":"What is my quota?"}]
        })
        .to_string();
        assert!(!is_quota_check_request(&body));
    }
}
