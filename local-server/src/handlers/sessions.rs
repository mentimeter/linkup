use axum::{Extension, Json, response::IntoResponse};
use http::StatusCode;
use linkup::{
    MemoryStringStore, NameKind, Session, SessionAllocator, SessionMode, UpsertSessionRequest,
};

use crate::{dns, handlers::ApiError};

pub async fn handle_upsert(
    Extension(store): Extension<MemoryStringStore>,
    Extension(dns_catalog): Extension<dns::DnsCatalog>,
    Json(upsert_req): Json<UpsertSessionRequest>,
) -> impl IntoResponse {
    let (desired_name, req_domains, mode) = match &upsert_req {
        UpsertSessionRequest::Named {
            desired_name,
            domains,
            mode,
            ..
        } => (desired_name.clone(), domains, mode.clone()),
        UpsertSessionRequest::Unnamed { domains, mode, .. } => {
            (String::new(), domains, mode.clone())
        }
    };

    let domains = req_domains
        .iter()
        .map(|domain| domain.domain.clone())
        .collect::<Vec<String>>();

    let server_conf: Session = match upsert_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - Local Server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    match mode {
        SessionMode::Tunneled => {
            let sessions = SessionAllocator::new(&store);
            let session_name_result = sessions
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

                dns::register_dns_record(&dns_catalog, &full_domain).await;
            }

            (StatusCode::OK, session_name).into_response()
        }
    }
}
