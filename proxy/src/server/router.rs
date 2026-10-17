use axum::http::StatusCode;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

/// 判斷請求是否帶有目前 Proxy token。
fn is_request_authorized(headers: &axum::http::HeaderMap, token: &str) -> bool {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let x_api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());

    free_claude_core::is_authorized_proxy_request(authorization, x_api_key, token)
}

/// 驗證受保護端點的本機 Proxy 憑證。
async fn require_proxy_authorization(request: Request, next: Next) -> Response {
    let authorized = free_claude_core::config_service::load_runtime_settings()
        .await
        .ok()
        .flatten()
        .is_some_and(|settings| {
            is_request_authorized(request.headers(), &settings.gateway.proxy_auth_token)
        });

    if authorized {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

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

    let protected_routes = Router::new()
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
        .route(
            "/v1/messages/count_tokens",
            post(super::handler::handle_count_tokens),
        )
        .route("/v1/models", get(super::models_endpoint::handle_models))
        .route_layer(middleware::from_fn(require_proxy_authorization));

    Router::new()
        .route("/", get(super::handler::handle_root))
        .route("/healthz", get(super::handler::handle_healthz))
        .route("/assets/icon.png", get(super::handler::handle_app_icon))
        .route("/dashboard", get(super::handler::handle_dashboard_page))
        .route("/dashboard.css", get(super::handler::handle_dashboard_css))
        .route("/dashboard.js", get(super::handler::handle_dashboard_js))
        .merge(protected_routes)
        .layer(DefaultBodyLimit::max(
            free_claude_core::constants::MAX_PROXY_BODY_BYTES,
        ))
        .layer(cors)
        .with_state(super::companion::CompanionState::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_routes_require_the_configured_proxy_token() {
        let mut headers = axum::http::HeaderMap::new();
        assert!(!is_request_authorized(&headers, "token"));

        headers.insert("x-api-key", "token".parse().unwrap());
        assert!(is_request_authorized(&headers, "token"));

        headers.insert("x-api-key", "wrong".parse().unwrap());
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer token".parse().unwrap(),
        );
        assert!(is_request_authorized(&headers, "token"));

        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer wrong".parse().unwrap(),
        );
        assert!(!is_request_authorized(&headers, "token"));
    }

    #[tokio::test]
    /// 驗證受保護端點需要憑證，且 CORS 只允許本機 Proxy 來源。
    async fn router_requires_token_and_restricts_cors_origins() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            axum::serve(listener, create_router(port)).await.unwrap();
        });
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::new();

        // 未帶憑證的受保護端點會被拒絕。
        let response = client.get(format!("{base}/settings")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // 指向本機 Proxy 埠的來源會通過 CORS 檢查。
        let response = client
            .get(format!("{base}/healthz"))
            .header("Origin", base.clone())
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            base
        );

        // 其他來源不會取得允許標頭。
        let response = client
            .get(format!("{base}/healthz"))
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );

        server.abort();
    }
}
