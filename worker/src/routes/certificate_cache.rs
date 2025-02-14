use axum::{
    extract::{self, State},
    response::IntoResponse,
    routing::put,
    Json, Router,
};
use http::StatusCode;

use crate::LinkupState;

pub fn router() -> Router<LinkupState> {
    Router::new().route(
        "/linkup/certificate-cache/{key}",
        put(upsert_certificate_cache_handler)
            .get(get_certificate_cache_handler)
            .delete(delete_certificate_cache_handler),
    )
}

#[worker::send]
async fn get_certificate_cache_handler(
    State(_state): State<LinkupState>,
    extract::Path(_key): extract::Path<String>,
) -> impl IntoResponse {
    (StatusCode::OK, "get_certificate_cache_handler stub").into_response()
}

#[worker::send]
async fn upsert_certificate_cache_handler(
    State(_state): State<LinkupState>,
    extract::Path(_key): extract::Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("update_certificate_cache_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn delete_certificate_cache_handler(
    State(_state): State<LinkupState>,
    extract::Path(_key): extract::Path<String>,
) -> impl IntoResponse {
    (StatusCode::OK, "delete_certificate_cache_handler stub").into_response()
}
