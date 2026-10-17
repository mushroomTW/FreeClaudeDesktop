use thiserror::Error;

/// 應用程式專屬的統一錯誤型別，基於 `thiserror` 實現。
#[derive(Debug, Error)]
pub enum AppError {
    // DPAPI 加密/解密錯誤
    #[error("Crypto error: {0}")]
    Crypto(String),

    // 設定檔與 IO 錯誤
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse/serialize error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Invalid configuration JSON: {0}")]
    InvalidConfigJson(#[source] serde_json::Error),

    // 啟動器與進程錯誤
    #[error("Launcher error: {0}")]
    Launcher(String),

    // 伺服器代理與 HTTP 錯誤
    #[error("Upstream request failed: {0}")]
    UpstreamRequest(#[from] reqwest::Error),
    #[error("Upstream API error (HTTP {status}): {body}")]
    UpstreamResponse { status: u16, body: String },
    #[error("Network proxy error: {0}")]
    Proxy(String),
}

/// 應用程式專屬的 Result 輔助型別。
pub type AppResult<T> = Result<T, AppError>;
