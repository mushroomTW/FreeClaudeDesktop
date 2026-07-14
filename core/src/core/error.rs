use thiserror::Error;

/// 應用程式專屬的統一錯誤型別，基於 `thiserror` 實現。
#[derive(Debug, Error)]
pub enum AppError {
    // DPAPI 加密/解密錯誤
    #[error("加解密錯誤: {0}")]
    Crypto(String),

    // 設定檔與 IO 錯誤
    #[error("I/O 錯誤: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 解析/序列化錯誤: {0}")]
    Json(#[from] serde_json::Error),
    #[error("無效設定: {0}")]
    InvalidConfig(String),
    #[error("無效設定 JSON: {0}")]
    InvalidConfigJson(#[source] serde_json::Error),

    // 啟動器與進程錯誤
    #[error("啟動錯誤: {0}")]
    Launcher(String),

    // 伺服器代理與 HTTP 錯誤
    #[error("上游請求失敗: {0}")]
    UpstreamRequest(#[from] reqwest::Error),
    #[error("上游 API 錯誤 (HTTP {status}): {body}")]
    UpstreamResponse { status: u16, body: String },
    #[error("網路代理錯誤: {0}")]
    Proxy(String),
}

/// 應用程式專屬的 Result 輔助型別。
pub type AppResult<T> = Result<T, AppError>;
