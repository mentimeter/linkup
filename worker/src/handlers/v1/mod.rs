pub mod local_session;
pub mod preview_session;
pub mod tunnel;

use axum::response::IntoResponse;
use http::StatusCode;
use linkup::{NameKind, Session, SessionAllocator, UpsertSessionRequest};

use crate::{http_error::HttpError, kv_store::CfWorkerStringStore, worker_state::WorkerState};

// TODO(augustoccesar)[2026-04-13]: This methods now exists because both the endpoints to
//  create a preview session and a local session are exactly the same with the only
//  difference being on the name generator kind.
//  We should probably deprecate them as separate endpoints and create a new one that
//  can take the name generator as part of the request.
pub async fn handle_session_upsert(
    state: WorkerState,
    req: UpsertSessionRequest,
    name_kind: NameKind,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let desired_name = match &req {
        UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
        UpsertSessionRequest::Unnamed { .. } => String::new(),
    };

    let session: Session = match req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - Worker", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let session_name = sessions
        .store_session(session, name_kind, &desired_name)
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return HttpError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    (StatusCode::OK, name).into_response()
}
