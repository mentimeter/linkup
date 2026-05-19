use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use http::StatusCode;
use linkup::{
    NameKind, Session, SessionDetailResponse, SessionError, SessionKind, SessionResponse,
    SessionsListResponse, UpsertSessionRequest,
};
use linkup_clients::WorkerClientError;

use crate::{ServerState, handlers::ApiError};

pub async fn list_sessions(State(server_state): State<ServerState>) -> impl IntoResponse {
    match server_state.session_allocator.list_sessions().await {
        Ok(sessions) => Json(SessionsListResponse {
            sessions: HashMap::from_iter(sessions),
        })
        .into_response(),
        Err(error) => ApiError::new(
            format!("Failed to list sessions: {}", error),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    }
}

pub async fn get_session(
    State(server_state): State<ServerState>,
    Path(session_name): Path<String>,
) -> impl IntoResponse {
    match server_state
        .session_allocator
        .find_session(&session_name)
        .await
    {
        Ok(Some(session)) => Json(SessionDetailResponse {
            session_kind: session.kind,
            session_name,
            services: session.services,
            domains: session.domains,
        })
        .into_response(),
        Ok(None) => ApiError::new(
            format!("Session '{}' not found", session_name),
            StatusCode::NOT_FOUND,
        )
        .into_response(),
        Err(error) => ApiError::new(
            format!("Failed to get session: {}", error),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    }
}

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
    log::debug!("Received tunneled session upsert request");

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

    let session: Session = match Session::from_upsert_req(SessionKind::Tunneled, upsert_req) {
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

    log::debug!(
        "Stored session on session store. session_kind={} session_name={}",
        session.kind.as_str(),
        tunneled_session.session_name
    );

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

        server_state.dns_catalog.register_record(&full_domain).await;
    }

    (StatusCode::OK, Json(tunneled_session)).into_response()
}

pub async fn upsert_isolated(
    State(server_state): State<ServerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let session: Session = match Session::from_upsert_req(SessionKind::Isolated, upsert_req.clone())
    {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let desired_name = match &upsert_req {
        UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
        UpsertSessionRequest::Unnamed { .. } => {
            return ApiError::new(
                "Isolated sessions should always be named".to_string(),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let isolated_session_result = server_state
        .session_allocator
        .strict_store_session(&desired_name, &session)
        .await;

    let session_name = match isolated_session_result {
        Ok(session_name) => session_name,
        Err(error) => match error {
            SessionError::EmptySessionName => {
                return ApiError::new(
                    "Isolated session name cannot be empty".to_string(),
                    StatusCode::BAD_REQUEST,
                )
                .into_response();
            }
            SessionError::SessionNameConflict => {
                return ApiError::new(
                    "Session name already exists and did not match secret".to_string(),
                    StatusCode::BAD_REQUEST,
                )
                .into_response();
            }
            _ => {
                return ApiError::new(
                    format!("Failed to store server session: {}", error),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        },
    };

    let domains = session
        .domains
        .iter()
        .map(|domain| domain.domain.clone())
        .collect::<Vec<String>>();

    for domain in &domains {
        let full_domain = format!("{session_name}.{domain}");

        server_state.dns_catalog.register_record(&full_domain).await;
    }

    let session_response = SessionResponse {
        session_name: session_name.to_string(),
    };

    (StatusCode::OK, Json(session_response)).into_response()
}

pub async fn delete_session(
    State(server_state): State<ServerState>,
    Path(session_name): Path<String>,
) -> impl IntoResponse {
    let session = match server_state
        .session_allocator
        .find_session(&session_name)
        .await
    {
        Ok(None) => {
            return ApiError::new(
                format!("Session '{}' not found", session_name),
                StatusCode::NOT_FOUND,
            )
            .into_response();
        }
        Err(error) => {
            return ApiError::new(
                format!("Failed to find session: {}", error),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
        Ok(Some(session)) => session,
    };

    if let Err(error) = server_state
        .session_allocator
        .delete_session(&session_name)
        .await
    {
        return ApiError::new(
            format!("Failed to delete session: {}", error),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response();
    }

    for domain in &session.domains {
        let full_domain = format!("{session_name}.{domain}", domain = domain.domain);

        server_state
            .dns_catalog
            .deregister_record(&full_domain)
            .await;
    }

    StatusCode::NO_CONTENT.into_response()
}
