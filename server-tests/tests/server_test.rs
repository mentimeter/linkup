use helpers::ServerKind;
use linkup::{CreatePreviewRequest, StorableDomain, StorableService};
use reqwest::Url;
use rstest::rstest;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[rstest]
#[tokio::test]
async fn can_respond_to_health_check(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind).await;

    let response = get(format!("{}/linkup/check", url)).await;

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
    let url = setup_server(server_kind).await;

    let response = get(format!("{}/linkup/local-session", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
}

#[rstest]
#[tokio::test]
async fn can_create_session(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind).await;

    let session_req = create_session_request("potatoname".to_string(), None);
    let response = post(format!("{}/linkup/local-session", url), session_req).await;

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
        .send()
        .await
        .expect("Failed to send request")
}

pub fn create_preview_request(fe_location: Option<String>) -> String {
    let location = match fe_location {
        Some(location) => location,
        None => "http://example.com".to_string(),
    };
    let req = CreatePreviewRequest {
        domains: vec![StorableDomain {
            domain: "example.com".to_string(),
            default_service: "frontend".to_string(),
            routes: None,
        }],
        services: vec![StorableService {
            name: "frontend".to_string(),
            location: Url::parse(&location).unwrap(),
            rewrites: None,
        }],
        cache_routes: None,
    };
    serde_json::to_string(&req).unwrap()
}
