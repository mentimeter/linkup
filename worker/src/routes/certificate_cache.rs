use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use http::StatusCode;

use crate::LinkupState;

pub fn router() -> Router<LinkupState> {
    Router::new().route(
        "/linkup/certificate-cache",
        get(get_certificate_cache_handler)
            .post(create_certificate_cache_handler)
            .put(update_certificate_cache_handler)
            .delete(delete_certificate_cache_handler),
    )
}

#[worker::send]
async fn get_certificate_cache_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "get_certificate_cache_handler stub").into_response()
}

#[worker::send]
async fn create_certificate_cache_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("create_certificate_cache_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn update_certificate_cache_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("update_certificate_cache_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn delete_certificate_cache_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "delete_certificate_cache_handler stub").into_response()
}
