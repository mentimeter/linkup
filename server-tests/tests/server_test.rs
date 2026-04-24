use helpers::ServerKind;
use linkup::{Domain, NameKind, SessionService, UpsertSessionRequest};
use reqwest::Url;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[tokio::test]
async fn can_respond_to_health_check() {
    let (url, _) = setup_server(ServerKind::Local).await;

    let response = get(format!("{}/linkup/check", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn no_such_session() {
    let (url, _) = setup_server(ServerKind::Local).await;

    let response = get(format!("{}/anypath", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
}

// The tests below require a running Worker instance (`npx wrangler@latest dev` in the worker dir).
// Run with: cargo test -p linkup-server-tests -- --include-ignored

#[tokio::test]
#[ignore = "requires running wrangler dev"]
async fn worker_can_respond_to_health_check() {
    let (url, _) = setup_server(ServerKind::Worker).await;

    let response = get(format!("{}/linkup/check", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires running wrangler dev"]
async fn worker_no_such_session() {
    let (url, _) = setup_server(ServerKind::Worker).await;

    let response = get(format!("{}/anypath", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
#[ignore = "requires running wrangler dev"]
async fn worker_can_create_session() {
    let (url, _) = setup_server(ServerKind::Worker).await;

    let session_req = create_session_request("potatoname".to_string(), None);
    let response = post(format!("{}/linkup/local-session", url), session_req).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "potatoname");
}

#[tokio::test]
#[ignore = "requires running wrangler dev"]
async fn worker_can_create_preview() {
    let (url, _) = setup_server(ServerKind::Worker).await;

    let session_req = create_preview_request(None);
    let response = post(format!("{}/linkup/preview-session", url), session_req).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap().len(), 6);
}

pub async fn get(url: String) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .get(url)
        .header("Authorization", "Bearer token123")
        // TODO(augustoccesar)[2025-02-24]: Proper test version header
        .header("x-linkup-version", "99.99.99")
        .send()
        .await
        .expect("Failed to send request")
}

pub fn create_preview_request(fe_location: Option<String>) -> String {
    let location = match fe_location {
        Some(location) => location,
        None => "http://example.com".to_string(),
    };
    let req = UpsertSessionRequest::Unnamed {
        name_kind: NameKind::SixChar,
        session_token: None,
        domains: vec![Domain {
            domain: "example.com".to_string(),
            default_service: "frontend".to_string(),
            routes: None,
        }],
        services: vec![SessionService {
            name: "frontend".to_string(),
            location: Url::parse(&location).unwrap(),
            rewrites: None,
        }],
        cache_routes: None,
    };
    serde_json::to_string(&req).unwrap()
}
