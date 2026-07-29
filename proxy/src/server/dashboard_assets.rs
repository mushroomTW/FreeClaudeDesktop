use axum::{
    http::header,
    response::{Html, IntoResponse},
};

/// 回傳 Dashboard 主頁。
pub async fn handle_dashboard_page() -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
            (header::PRAGMA, "no-cache"),
            (header::EXPIRES, "0"),
        ],
        Html(include_str!("dashboard.html")),
    )
}

/// 回傳 Dashboard 頁面的 CSS 資源。
pub async fn handle_dashboard_css() -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        include_str!("dashboard.css"),
    )
}

/// 回傳 Dashboard 頁面的 JavaScript 資源。
pub async fn handle_dashboard_js() -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-cache, no-store, must-revalidate"),
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
        ],
        include_str!("dashboard.js"),
    )
}
