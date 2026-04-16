use axum::{Json, extract::State, response::IntoResponse};
use linkup::NameKind;

use crate::worker_state::WorkerState;

#[worker::send]
pub async fn handle_post(
    State(state): State<WorkerState>,
    Json(upsert_req): Json<super::UpsertSessionRequest>,
) -> impl IntoResponse {
    super::handle_session_upsert(state, upsert_req, NameKind::SixChar).await
}
