use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[tokio::test]
async fn can_respond_to_health_check() {
    let url = setup_server().await;

    let response = get(format!("{}/linkup-check", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn no_such_session() {
    let url = setup_server().await;

    let response = get(format!("{}/anypath", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn method_not_allowed_config_get() {
    let url = setup_server().await;

    let response = get(format!("{}/linkup", url)).await;

    assert_eq!(response.status(), reqwest::StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn can_create_session() {
    let url = setup_server().await;

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
