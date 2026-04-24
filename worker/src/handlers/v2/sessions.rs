use axum::{Json, extract::State, response::IntoResponse};

use http::StatusCode;
use linkup::{
    NameKind, Session, SessionError, SessionResponse, TunneledSessionResponse, UpsertSessionRequest,
};

use crate::{http_error::HttpError, tunnel, worker_state::WorkerState};

#[worker::send]
pub async fn upsert_preview(
    State(state): State<WorkerState>,
    Json(req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let session: Session = match req.clone().try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - Worker", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    for service in &session.services {
        if let Some(host) = service.location.host()
            && &host.to_string() == "localhost"
        {
            return HttpError::new(
                "Preview session cannot contain services pointing to localhost".to_string(),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    }

    let session_name = match &req {
        UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
        UpsertSessionRequest::Unnamed { .. } => {
            let desired_name = state
                .session_allocator
                .new_session_name(&NameKind::Animal, "", &session)
                .await;

            match desired_name {
                Ok(desired_name) => desired_name,
                Err(error) => {
                    return HttpError::new(
                        format!("Failed generate new session name: {}", error),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response();
                }
            }
        }
    };

    if let Err(error) = state
        .session_allocator
        .strict_store_session(&session_name, &session)
        .await
    {
        match error {
            SessionError::SessionNameConflict => {
                return HttpError::new("Conflict".to_string(), StatusCode::CONFLICT)
                    .into_response();
            }
            _ => {
                return HttpError::new(
                    format!("Failed to store server config: {}", error),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        }
    }

    (StatusCode::OK, Json(SessionResponse { session_name })).into_response()
}

#[worker::send]
pub async fn upsert_tunneled(
    State(state): State<WorkerState>,
    Json(req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let mut session: Session = match req.clone().try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - Worker", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let desired_name = match &req {
        UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
        UpsertSessionRequest::Unnamed { .. } => {
            let desired_name = state
                .session_allocator
                .new_session_name(&NameKind::Animal, "", &session)
                .await;

            match desired_name {
                Ok(desired_name) => desired_name,
                Err(error) => {
                    return HttpError::new(
                        format!("Failed generate new session name: {}", error),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response();
                }
            }
        }
    };

    let tunnel_data = match tunnel::upsert_tunnel(&state, &desired_name).await {
        Ok(data) => data,
        Err(e) => {
            return HttpError::new(
                format!("Failed to upsert tunnel: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    for service in session.services.iter_mut() {
        if let Some(host) = service.location.host()
            && &host.to_string() == "localhost"
        {
            service.location = tunnel_data
                .url
                .parse()
                .expect("tunnel url should be valid URL");
        }
    }

    if let Err(error) = state
        .session_allocator
        .strict_store_session(&desired_name, &session)
        .await
    {
        match error {
            SessionError::SessionNameConflict => {
                return HttpError::new(
                    format!("Failed to store server config: {}", error),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
            _ => {
                return HttpError::new("Conflict".to_string(), StatusCode::CONFLICT)
                    .into_response();
            }
        }
    }

    let response = TunneledSessionResponse {
        session_name: desired_name,
        tunnel_data,
    };

    (StatusCode::OK, Json(response)).into_response()
}
