use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

pub fn create_router(port: u16) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            origin.to_str().ok().is_some_and(|origin| {
                crate::conversion::response_converter::is_allowed_origin(Some(origin), port)
            })
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(super::handler::handle_root))
        .route("/healthz", get(super::handler::handle_healthz))
        .route("/assets/icon.png", get(super::handler::handle_app_icon))
        .route("/admin", get(super::handler::handle_admin_page))
        .route(
            "/settings",
            get(super::handler::handle_admin_settings).post(super::handler::update_admin_settings),
        )
        .route("/status", get(super::handler::handle_admin_status))
        .route("/rpc", post(super::handler::handle_admin_rpc))
        .route(
            "/companion",
            get(super::handler::handle_companion_websocket),
        )
        .route("/v1/messages", post(super::handler::handle_proxy))
        .route("/v1/models", get(super::models_endpoint::handle_models))
        .route(
            "/__launcher_show",
            get(super::handler::handle_launcher_show),
        )
        .layer(DefaultBodyLimit::max(
            crate::constants::MAX_PROXY_BODY_BYTES,
        ))
        .layer(cors)
}
