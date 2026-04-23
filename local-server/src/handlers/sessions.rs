use axum::{Json, extract::State, response::IntoResponse};
use http::StatusCode;
use linkup::{NameKind, Session, UpsertSessionRequest};
use linkup_clients::WorkerClientError;

use crate::{ServerState, dns, handlers::ApiError};

pub async fn upsert_preview(
    State(server_state): State<ServerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    match server_state
        .worker_client
        .preview_session(&upsert_req)
        .await
    {
        Ok(session_response) => Json(session_response).into_response(),
        Err(error) => match error {
            WorkerClientError::Response(status_code, message) => {
                ApiError::new(message, status_code).into_response()
            }
            _ => ApiError::new(
                format!("Failed to request to Worker: {}", error),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response(),
        },
    }
}

pub async fn upsert_tunneled(
    State(server_state): State<ServerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let tunneled_session = match server_state
        .worker_client
        .tunneled_session(&upsert_req)
        .await
    {
        Ok(tunneled_session) => tunneled_session,
        Err(error) => match error {
            WorkerClientError::Response(StatusCode::CONFLICT, _) => {
                return ApiError::new("Conflict".to_string(), StatusCode::CONFLICT).into_response();
            }
            _ => {
                return ApiError::new(
                    format!("Failed to request to Worker: {}", error),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        },
    };

    let session: Session = match upsert_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let local_session_result = server_state
        .session_allocator
        .store_session(&session, NameKind::Animal, &tunneled_session.session_name)
        .await;

    if let Err(error) = local_session_result {
        return ApiError::new(
            format!("Failed to store server config: {}", error),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response();
    }

    let domains = session
        .domains
        .iter()
        .map(|domain| domain.domain.clone())
        .collect::<Vec<String>>();

    for domain in &domains {
        let full_domain = format!(
            "{session_name}.{domain}",
            session_name = tunneled_session.session_name
        );

        dns::register_dns_record(&server_state.dns_catalog, &full_domain).await;
    }

    (StatusCode::OK, Json(tunneled_session)).into_response()
}

pub async fn upsert_local_only(
    State(_server_state): State<ServerState>,
    Json(_upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    // Local work only.
    StatusCode::NOT_FOUND
}
