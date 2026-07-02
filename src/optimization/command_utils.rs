//! Command parsing utilities for API optimizations.

use regex::Regex;

/// Extract the command prefix for fast prefix detection.
///
/// Parses a shell command safely, handling environment variables and
/// avoiding command injection.
///
/// # Examples
/// ```
/// # use free_claude_launcher::optimization::command_utils::parse_shell_command_prefix;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_commit() {
        assert_eq!(
            parse_shell_command_prefix("git commit -m 'hello'"),
            "git commit"
        );
    }

    #[test]
    fn test_parse_docker_build() {
        assert_eq!(
            parse_shell_command_prefix("docker build -t myapp ."),
            "docker build"
        );
        assert_eq!(parse_shell_command_prefix("docker ps"), "docker ps");
    }

    #[test]
    fn test_env_vars() {
        assert_eq!(
            parse_shell_command_prefix("ENV=prod npm install"),
            "npm install"
        );
    }

    #[test]
    fn test_command_injection() {
        assert_eq!(
            parse_shell_command_prefix("`rm -rf /`"),
            "command_injection_detected"
        );
    }
}
