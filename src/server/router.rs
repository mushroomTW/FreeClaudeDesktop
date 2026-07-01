use axum::{
    routing::{get, post},
    Router,
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
        .route("/v1/messages", post(super::handler::handle_proxy))
        .route("/v1/models", get(super::models_endpoint::handle_models))
        .route(
            "/__launcher_show",
            get(super::handler::handle_launcher_show),
        )
        .layer(cors)
}
