use helpers::ServerKind;
use rstest::rstest;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[rstest]
#[tokio::test]
async fn can_respond_to_health_check(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind).await;

    let response = get(format!("{}/linkup-check", url)).await;

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

    let response = get(format!("{}/linkup", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
}

#[rstest]
#[tokio::test]
async fn can_create_session(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind).await;

    let session_req = create_session_request("potatoname".to_string(), None);
    let response = post(format!("{}/linkup", url), session_req).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "potatoname");
}

pub async fn get(url: String) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .get(url)
        .send()
        .await
        .expect("Failed to send request")
}
