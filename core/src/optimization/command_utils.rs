//! Command parsing utilities for API optimizations.

/// Extract the command prefix for fast prefix detection.
///
/// Parses a shell command safely, handling environment variables and
/// avoiding command injection.
///
/// # Examples
/// ```
/// # use free_claude_core::optimization::command_utils::parse_shell_command_prefix;
/// assert_eq!(parse_shell_command_prefix("git commit -m 'hello'"), "git commit");
/// ```
pub fn parse_shell_command_prefix(command: &str) -> String {
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

/// 正規化 `strip_env_assignments` 所處理的資料。
pub(crate) fn strip_env_assignments<'a>(parts: &[&'a str]) -> Vec<&'a str> {
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

/// 判斷是否符合 `is_env_name_char` 的條件。
fn is_env_name_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// 判斷是否符合 `is_env_assignment` 的條件。
pub(crate) fn is_env_assignment(part: &str) -> bool {
    let Some((name, _)) = part.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(is_env_name_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 驗證 `test_parse_git_commit` 的行為符合預期。
    fn test_parse_git_commit() {
        assert_eq!(
            parse_shell_command_prefix("git commit -m 'hello'"),
            "git commit"
        );
    }

    #[test]
    /// 驗證 `test_parse_docker_build` 的行為符合預期。
    fn test_parse_docker_build() {
        assert_eq!(
            parse_shell_command_prefix("docker build -t myapp ."),
            "docker build"
        );
        assert_eq!(parse_shell_command_prefix("docker ps"), "docker ps");
    }

    #[test]
    /// 驗證 `test_env_vars` 的行為符合預期。
    fn test_env_vars() {
        assert_eq!(
            parse_shell_command_prefix("ENV=prod npm install"),
            "npm install"
        );
    }

    #[test]
    /// 驗證 `test_command_injection` 的行為符合預期。
    fn test_command_injection() {
        assert_eq!(
            parse_shell_command_prefix("`rm -rf /`"),
            "command_injection_detected"
        );
    }
}
