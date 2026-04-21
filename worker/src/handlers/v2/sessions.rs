use axum::{Json, extract::State, response::IntoResponse};
use http::StatusCode;

use linkup::{Session, SessionAllocator, UpsertSessionRequest};

use crate::{http_error::HttpError, kv_store::CfWorkerStringStore, worker_state::WorkerState};

#[worker::send]
pub async fn upsert_preview(
    State(state): State<WorkerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    // Create session, but don't create tunnel infrastructure.
    // TODO(@augustoccesar)[2026-04-21]: Reject any service with localhost
}

#[worker::send]
pub async fn upsert_tunneled(
    State(state): State<WorkerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    // Create session and tunnel infrastructure.
    // TODO(@augustoccesar)[2026-04-21]: remember to convert localhost's into tunnel url. This was done before by the CLI
}

// pub async fn handle_post(state: WorkerState, req: UpsertSessionRequest) -> impl IntoResponse {
//     let store = CfWorkerStringStore::new(state.sessions_kv.clone());
//     let sessions = SessionAllocator::new(&store);

//     let session: Session = match req.try_into() {
//         Ok(conf) => conf,
//         Err(e) => {
//             return HttpError::new(
//                 format!("Failed to parse server config: {} - Worker", e),
//                 StatusCode::BAD_REQUEST,
//             )
//             .into_response();
//         }
//     };

//     let desired_name = match &req {
//         UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
//         UpsertSessionRequest::Unnamed { name_kind, .. } => {
//             // TODO(@augustoccesar)[2026-04-20]: Remove unwrap
//             sessions
//                 .new_session_name(name_kind, "", &session)
//                 .await
//                 .unwrap()
//         }
//     };

//     // let session_name = sessions
//     //     .store_session(session, name_kind, &desired_name)
//     //     .await;

//     // let name = match session_name {
//     //     Ok(session_name) => session_name,
//     //     Err(e) => {
//     //         return HttpError::new(
//     //             format!("Failed to store server config: {}", e),
//     //             StatusCode::INTERNAL_SERVER_ERROR,
//     //         )
//     //         .into_response();
//     //     }
//     // };

//     // (StatusCode::OK, name).into_response()

//     StatusCode::OK
// }
