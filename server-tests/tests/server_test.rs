use helpers::ServerKind;
use linkup::{Domain, NameKind, SessionMode, SessionService, UpsertSessionRequest};
use reqwest::Url;
use rstest::rstest;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[rstest]
#[tokio::test]
async fn can_respond_to_health_check(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind.clone()).await;

    let response = match server_kind {
        ServerKind::Local => get(format!("{}/linkup/health/ping", url)).await,
        ServerKind::Worker => get(format!("{}/linkup/check", url)).await,
    };

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[rstest]
#[tokio::test]
async fn no_such_session(#[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind) {
    let url = setup_server(server_kind).await;

    let response = get(format!("{}/anypath", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
}

#[rstest]
#[tokio::test]
async fn method_not_allowed_config_get(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind.clone()).await;

    let response = match server_kind {
        ServerKind::Local => get(format!("{}/linkup/sessions", url)).await,
        ServerKind::Worker => get(format!("{}/linkup/local-session", url)).await,
    };

    assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
}

#[rstest]
#[tokio::test]
async fn can_create_session(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind.clone()).await;

    let session_req = create_session_request("potatoname".to_string(), None);

    let response = match server_kind {
        ServerKind::Local => post(format!("{}/linkup/sessions", url), session_req).await,
        ServerKind::Worker => post(format!("{}/linkup/local-session", url), session_req).await,
    };

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "potatoname");
}

#[rstest]
#[tokio::test]
async fn can_create_preview(#[values(ServerKind::Worker)] server_kind: ServerKind) {
    let url = setup_server(server_kind).await;

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
        mode: SessionMode::Tunneled,
        name_kind: NameKind::SixChar,
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
