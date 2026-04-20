use std::path::Path;

use axum::{Extension, Json, response::IntoResponse};
use http::StatusCode;

use linkup::{
    MemoryStringStore, NameKind, Session, SessionAllocator, SessionMode, UpsertSessionRequest,
};
use linkup_clients::WorkerClient;
use url::Url;

use crate::{dns, handlers::ApiError};

pub async fn handle_upsert(
    Extension(store): Extension<MemoryStringStore>,
    Extension(dns_catalog): Extension<dns::DnsCatalog>,
    Extension(worker_url): Extension<Url>,
    Extension(worker_token): Extension<String>,
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

    // TODO: Make this nicer so Soph don't scream
    let upser_req_clone = upsert_req.clone();
    let mut server_conf: Session = match upsert_req.try_into() {
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
            let worker_client = WorkerClient::new(&worker_url, &worker_token);
            // TODO: Change so that the remote fails if there is conflict instead of giving a new name
            //  Then handle it here and on the client.
            let worker_session_name = worker_client.local_session(&upser_req_clone).await.unwrap();
            // let tunnel = worker_client
            //     .get_tunnel(&worker_session_name)
            //     .await
            //     .unwrap();

            if worker_session_name != desired_name {
                println!("Worker name mismatch requested name");
            }

            let sessions = SessionAllocator::new(&store);
            let session_name_result = sessions
                .store_session(server_conf, NameKind::Animal, &worker_session_name)
                .await;

            let session_name = match session_name_result {
                Ok(local_server_session_name) => {
                    if local_server_session_name != worker_session_name {
                        return ApiError::new(
                        format!("Session name mismatch: Worker gave '{}', and Local Server gave '{}'", worker_session_name, local_server_session_name),
                        StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response();
                    }

                    local_server_session_name
                }
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
