use axum::{routing::any, Router};
use tokio::net::TcpListener;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[tokio::test]
async fn can_request_underlying_server() {
    let url = setup_server().await;
    let underlying_url = setup_underlying_server("under_fe".to_string()).await;

    let session_req = create_session_request("potatosession".to_string(), Some(underlying_url));
    let session_resp = post(format!("{}/linkup", url), session_req).await;
    assert_eq!(session_resp.status(), reqwest::StatusCode::OK);
    assert_eq!(session_resp.text().await.unwrap(), "potatosession");

    let response = get_session(
        format!("{}/anypath", url),
        "example.com".to_string(),
        "potatosession".to_string(),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "under_fe");
}

async fn setup_underlying_server(name: String) -> String {
    let app = Router::new().fallback(any(|| async { name }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}", addr)
}

async fn get_session(url: String, destination: String, session_name: String) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .get(url)
        .header("traceparent", "xzyabc")
        .header("tracestate", format!("linkup-session={}", session_name))
        .header("Referer", destination)
        .send()
        .await
        .expect("Failed to send request")
}
