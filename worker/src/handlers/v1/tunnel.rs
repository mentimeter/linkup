use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use http::StatusCode;
use serde::Deserialize;

use crate::{http_error::HttpError, tunnel, worker_state::WorkerState};

#[derive(Deserialize)]
pub struct GetTunnelParams {
    session_name: String,
}

#[worker::send]
pub async fn handle_get(
    State(state): State<WorkerState>,
    Query(query): Query<GetTunnelParams>,
) -> impl IntoResponse {
    match tunnel::upsert_tunnel(&state, &query.session_name).await {
        Ok(tunnel_data) => Json(tunnel_data).into_response(),
        Err(e) => HttpError::new(
            format!("Failed to upsert tunnel: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    }
}
