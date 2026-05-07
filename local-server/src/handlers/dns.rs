use axum::{Json, extract::State, response::IntoResponse};
use linkup::DnsListResponse;

use crate::ServerState;

pub async fn list(State(server_state): State<ServerState>) -> impl IntoResponse {
    Json(DnsListResponse {
        domains: server_state.dns_catalog.list_domains().await,
    })
}
