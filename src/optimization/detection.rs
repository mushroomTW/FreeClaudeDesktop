//! Request detection utilities for local optimizations.
//!
//! Detects quota checks, prefix commands, title generation, safety classifier,
//! suggestion mode, and filepath extraction requests.

use regex::Regex;
use serde_json::Value;

/// Check if this is a quota probe request.
///
/// Quota checks are typically simple requests with max_tokens=1
/// and a single message containing the word "quota".
pub fn is_quota_check_request(body_str: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };

    let max_tokens = v.get("max_tokens").and_then(Value::as_u64);
    let messages = v.get("messages").and_then(Value::as_array);

    if max_tokens != Some(1) {
        return false;
    }
    if messages.map(|m| m.len()) != Some(1) {
        return false;
    }

    let text = messages
        .and_then(|arr| arr.first())
        .and_then(|msg| msg.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    text.to_lowercase().contains("quota")
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

    let system_text = match v.get("system") {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => return false,
    };

    if !system_text.contains("title") {
        return false;
    }

    system_text.contains("sentence-case title")
        || (system_text.contains("return json")
            && system_text.contains("field")
            && (system_text.contains("coding session") || system_text.contains("this session")))
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

    let system_text = v
        .get("system")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_lowercase();

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
    let output = output
        .split(|c| c == '<' || c == '\n')
        .next()
        .unwrap_or(output)
        .trim();

    let filepaths = extract_filepaths_from_command(command, output);
    Some(filepaths)
}

/// Check if this is a safety classifier request.
///
/// Detects Claude Code's auto-mode safety classifier prompt.
pub fn is_safety_classifier_request(body_str: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(body_str) else {
        return false;
    };

    if v.get("tools").is_some() {
        return false;
    }

    let system_text = v.get("system").and_then(|s| s.as_str()).unwrap_or("");
    let messages_text: String = v
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|msg| msg.get("content").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let combined = format!("{}\n{}", system_text, messages_text).to_lowercase();
    let has_verdict = combined.contains("yes</block>") || combined.contains("no</block>");
    combined.contains("<transcript>") && has_verdict
}

/// Parse a shell command to extract the command prefix.
fn parse_shell_command_prefix(command: &str) -> String {
    let command = command.trim();
    if command.contains("`") || command.contains("$(") {
        return "command_injection_detected".to_string();
    }

    let parts: Vec<&str> = command.split_whitespace().collect();
    let parts = strip_env_assignments(&parts);

    if parts.is_empty() {
        return "none".to_string();
    }

    let first = parts[0];
    let two_word_cmds = [
        "git", "npm", "docker", "kubectl", "cargo", "go", "pip", "yarn",
    ];

    if two_word_cmds.contains(&first) && parts.len() > 1 {
        let second = parts[1];
        if !second.starts_with('-') {
            return format!("{} {}", first, second);
        }
        return first.to_string();
    }

    first.to_string()
}

fn strip_env_assignments<'a>(parts: &[&'a str]) -> Vec<&'a str> {
    let mut start = 0;
    for (i, part) in parts.iter().enumerate() {
        if is_env_assignment(part) {
            start = i + 1;
        } else {
            break;
        }
    }
    parts[start..].to_vec()
}

fn is_env_assignment(part: &str) -> bool {
    let re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*=.*$").unwrap();
    re.is_match(part)
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
