use axum::{
    Json,
    body::{Body, Bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use free_claude_core::optimization::OptimizationResponse;

/// 將核心層的最佳化結果轉為 Axum HTTP 回應。
pub fn into_response(result: OptimizationResponse) -> Response {
    match result {
        OptimizationResponse::Json(value) => (StatusCode::OK, Json(value)).into_response(),
        OptimizationResponse::Sse(events) => {
            let stream = futures::stream::iter(
                events
                    .into_iter()
                    .map(|event| Ok::<_, std::convert::Infallible>(Bytes::from(event))),
            );

            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/event-stream; charset=utf-8")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(Body::from_stream(stream))
                .expect("固定的最佳化回應標頭必須有效")
        }
    }
}
