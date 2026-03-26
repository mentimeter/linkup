use axum::{response::IntoResponse, Extension, Json};
use http::StatusCode;
use linkup::{MemoryStringStore, NameKind, Session, SessionAllocator, UpdateSessionRequest};

use crate::linkup_server::ApiError;

pub async fn handle_upsert(
    Extension(store): Extension<MemoryStringStore>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let desired_name = update_req.desired_name.clone();
    let server_conf: Session = match update_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let sessions = SessionAllocator::new(&store);
    let session_name = sessions
        .store_session(server_conf, NameKind::Animal, desired_name)
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return ApiError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };

    (StatusCode::OK, name).into_response()
}
