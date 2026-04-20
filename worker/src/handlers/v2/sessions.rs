use axum::{Json, extract::State, response::IntoResponse};

use http::StatusCode;
use linkup::{Session, SessionAllocator, UpsertSessionRequest};
use url::Url;

use crate::{http_error::HttpError, kv_store::CfWorkerStringStore, worker_state::WorkerState};

#[worker::send]
pub async fn handle_post(
    State(state): State<WorkerState>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    // let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    // let sessions = SessionAllocator::new(&store);

    // let desired_name = match &upsert_req {
    //     UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
    //     UpsertSessionRequest::Unnamed { .. } => String::new(),
    // };

    // let mut session: Session = match upsert_req.try_into() {
    //     Ok(conf) => conf,
    //     Err(e) => {
    //         return HttpError::new(
    //             format!("Failed to parse server config: {} - Worker", e),
    //             StatusCode::BAD_REQUEST,
    //         )
    //         .into_response();
    //     }
    // };

    // let tunnel = crate::tunnel::upsert_tunnel(state, &desired_name)
    //     .await
    //     .unwrap();

    // for service in session.services.iter_mut() {
    //     if let Some(host) = service.location.host()
    //         && &host.to_string() == "localhost"
    //     {
    //         service.location = Url::parse(&tunnel.url).unwrap();
    //     }
    // }

    // let session_name = sessions
    //     .store_session(session, name_kind, &desired_name)
    //     .await;

    // let name = match session_name {
    //     Ok(session_name) => session_name,
    //     Err(e) => {
    //         return HttpError::new(
    //             format!("Failed to store server config: {}", e),
    //             StatusCode::INTERNAL_SERVER_ERROR,
    //         )
    //         .into_response();
    //     }
    // };

    // (StatusCode::OK, name).into_response()
    StatusCode::NOT_FOUND
}
