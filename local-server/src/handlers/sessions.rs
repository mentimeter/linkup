use axum::{Json, extract::State, response::IntoResponse};
use http::StatusCode;
use linkup::{NameKind, Session, UpsertSessionRequest};

use crate::{ServerState, dns, handlers::ApiError};

pub async fn handle_upsert(
    State(server_state): State<ServerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let (desired_name, req_domains) = match &upsert_req {
        UpsertSessionRequest::Named {
            desired_name,
            domains,
            ..
        } => (desired_name.clone(), domains),
        UpsertSessionRequest::Unnamed { domains, .. } => (String::new(), domains),
    };

    let domains = req_domains
        .iter()
        .map(|domain| domain.domain.clone())
        .collect::<Vec<String>>();

    let server_conf: Session = match upsert_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let session_name_result = server_state
        .session_allocator
        .store_session(server_conf, NameKind::Animal, &desired_name)
        .await;

    let session_name = match session_name_result {
        Ok(session_name) => session_name,
        Err(e) => {
            return ApiError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    for domain in &domains {
        let full_domain = format!("{session_name}.{domain}");

        dns::register_dns_record(&server_state.dns_catalog, &full_domain).await;
    }

    (StatusCode::OK, session_name).into_response()
}
