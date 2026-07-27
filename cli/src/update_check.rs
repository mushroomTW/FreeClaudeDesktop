use std::time::Duration;

#[derive(Debug, serde::Serialize)]
pub(crate) struct UpdateCheck {
    pub(crate) current_version: &'static str,
    pub(crate) latest_version: String,
    pub(crate) update_available: bool,
    pub(crate) release_url: String,
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

/// 查詢 GitHub 最新版本並產生可序列化的更新狀態。
pub(crate) async fn check_for_update() -> Result<UpdateCheck, Box<dyn std::error::Error>> {
    let release = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("freeclaude-cli")
        .build()?
        .get("https://api.github.com/repos/mushroomTW/FreeClaudeDesktop/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json::<Release>()
        .await?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();

    Ok(UpdateCheck {
        current_version: env!("CARGO_PKG_VERSION"),
        update_available: version_is_newer(&latest_version, env!("CARGO_PKG_VERSION")),
        latest_version,
        release_url: release.html_url,
    })
}

/// 比較固定三段式語意版本；格式無效時視為沒有更新。
pub(crate) fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn parts(value: &str) -> Option<[u64; 3]> {
        let mut parts = value.split('.').map(str::parse::<u64>);
        Some([
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        ])
    }

    match (parts(candidate), parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}
