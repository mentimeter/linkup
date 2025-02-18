// TODO(augustoccesar)[2025-02-14]: Handle errors instead of using .unwrap()

use axum::{
    extract::{self, State},
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use base64::prelude::*;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use worker::console_log;

use crate::LinkupState;

pub fn router() -> Router<LinkupState> {
    Router::new()
        .route(
            "/linkup/certificate-cache/keys",
            get(list_certificate_cache_keys_handler),
        )
        .route(
            "/linkup/certificate-cache/{key}",
            put(upsert_certificate_cache_handler)
                .get(get_certificate_cache_handler)
                .delete(delete_certificate_cache_handler),
        )
}

#[worker::send]
async fn list_certificate_cache_keys_handler(
    State(state): State<LinkupState>,
) -> impl IntoResponse {
    // TODO(augustoccesar)[2025-02-17]: Add pagination here. We should be fine with 1000 for now, but might be a problem in the future.
    Json(
        state
            .certs_kv
            .list()
            .limit(1000)
            .execute()
            .await
            .unwrap()
            .keys
            .iter()
            .map(|key| key.name.clone())
            .collect::<Vec<String>>(),
    )
}

#[derive(Debug, Serialize)]
struct CertificateCacheResponse {
    data_base64: String,
    size: usize,
    last_modified: u64,
}

#[derive(Serialize, Deserialize)]
struct CertificateMetadata {
    last_modified: u64,
}

#[worker::send]
async fn get_certificate_cache_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
) -> impl IntoResponse {
    let (data, metadata) = state
        .certs_kv
        .get(&key)
        .bytes_with_metadata::<String>()
        .await
        .unwrap();

    match data {
        Some(data) => {
            let data_base64 = BASE64_STANDARD.encode(&data);
            let last_modified = metadata.map_or_else(
                || worker::Date::now().as_millis(),
                |m| {
                    serde_json::from_str::<'_, CertificateMetadata>(&m)
                        .unwrap()
                        .last_modified
                },
            );

            Json(CertificateCacheResponse {
                data_base64,
                size: data.len(),
                last_modified,
            })
            .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct UpsertCertificateCachePayload {
    data_base64: String,
}

#[worker::send]
async fn upsert_certificate_cache_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
    Json(payload): Json<UpsertCertificateCachePayload>,
) -> impl IntoResponse {
    let data = BASE64_STANDARD.decode(&payload.data_base64).unwrap();
    let metadata = CertificateMetadata {
        last_modified: worker::Date::now().as_millis(),
    };

    let req = state
        .certs_kv
        .put_bytes(&key, &data)
        .unwrap()
        .metadata(serde_json::to_string(&metadata).unwrap())
        .unwrap();

    console_log!("Payload: {}", serde_json::to_string(&req).unwrap());

    req.execute().await.unwrap();

    Json(CertificateCacheResponse {
        data_base64: payload.data_base64,
        size: data.len(),
        last_modified: metadata.last_modified,
    })
}

#[worker::send]
async fn delete_certificate_cache_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
) -> impl IntoResponse {
    state.certs_kv.delete(&key).await.unwrap();

    StatusCode::NO_CONTENT.into_response()
}
