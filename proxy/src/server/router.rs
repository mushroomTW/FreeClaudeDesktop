use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

/// 建立 `create_router` 所需的結果。
pub fn create_router(port: u16) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            origin.to_str().ok().is_some_and(|origin| {
                free_claude_core::conversion::response_converter::is_allowed_origin(
                    Some(origin),
                    port,
                )
            })
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(super::handler::handle_root))
        .route("/healthz", get(super::handler::handle_healthz))
        .route("/assets/icon.png", get(super::handler::handle_app_icon))
        .route("/dashboard", get(super::handler::handle_dashboard_page))
        .route("/dashboard.css", get(super::handler::handle_dashboard_css))
        .route("/dashboard.js", get(super::handler::handle_dashboard_js))
        .route(
            "/settings",
            get(super::handler::handle_dashboard_settings)
                .post(super::handler::update_dashboard_settings),
        )
        .route("/status", get(super::handler::handle_dashboard_status))
        .route("/rpc", post(super::handler::handle_dashboard_rpc))
        .route(
            "/companion",
            get(super::handler::handle_companion_websocket),
        )
        .route("/v1/messages", post(super::handler::handle_proxy))
        .route("/v1/models", get(super::models_endpoint::handle_models))
        .layer(DefaultBodyLimit::max(
            free_claude_core::constants::MAX_PROXY_BODY_BYTES,
        ))
        .layer(cors)
        .with_state(super::companion::CompanionState::default())
}
