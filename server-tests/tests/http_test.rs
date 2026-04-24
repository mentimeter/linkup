use axum::{
    Router,
    response::{AppendHeaders, Redirect},
    routing::{any, get},
};
use http::{StatusCode, header::SET_COOKIE};
use tokio::net::TcpListener;

use crate::helpers::{ServerKind, seed_session, setup_server};

mod helpers;

#[tokio::test]
async fn can_request_underlying_server() {
    let (url, allocator) = setup_server(ServerKind::Local).await;
    let underlying_url = setup_underlying_server("under_fe".to_string()).await;

    seed_session(
        allocator.as_ref().unwrap(),
        "potatosession",
        &underlying_url,
    )
    .await;

    let response = get_session(
        format!("{}/anypath", url),
        "example.com".to_string(),
        "potatosession".to_string(),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "under_fe");
}

#[tokio::test]
async fn does_not_follow_redirects() {
    let (url, allocator) = setup_server(ServerKind::Local).await;
    let underlying_url = setup_underlying_server("under_fe".to_string()).await;

    seed_session(
        allocator.as_ref().unwrap(),
        "potatosession",
        &underlying_url,
    )
    .await;

    let response = get_session(
        format!("{}/redirect", url),
        "example.com".to_string(),
        "potatosession".to_string(),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/somethingelse"
    );
}

#[tokio::test]
async fn maintains_multiple_set_cookie_headers() {
    let (url, allocator) = setup_server(ServerKind::Local).await;
    let underlying_url = setup_underlying_server("under_fe".to_string()).await;

    seed_session(
        allocator.as_ref().unwrap(),
        "potatosession",
        &underlying_url,
    )
    .await;

    let response = get_session(
        format!("{}/cookies", url),
        "example.com".to_string(),
        "potatosession".to_string(),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let cookies: Vec<_> = response.headers().get_all("set-cookie").iter().collect();
    assert_eq!(cookies.len(), 2);
    assert_eq!(cookies[0].to_str().unwrap(), "cookie1=value1; Path=/");
    assert_eq!(cookies[1].to_str().unwrap(), "cookie2=value2; Path=/");
}

async fn setup_underlying_server(name: String) -> String {
    let app = Router::new()
        .route("/redirect", get(Redirect::temporary("/somethingelse")))
        .route(
            "/cookies",
            get(|| async {
                (
                    StatusCode::OK,
                    AppendHeaders([
                        (SET_COOKIE, "cookie1=value1; Path=/"),
                        (SET_COOKIE, "cookie2=value2; Path=/"),
                    ]),
                )
            }),
        )
        .fallback(any(|| async { name }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}", addr)
}

async fn get_session(url: String, destination: String, session_name: String) -> reqwest::Response {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Failed to build client");

    client
        .get(url)
        .header("traceparent", "xzyabc")
        .header("tracestate", format!("linkup-session={}", session_name))
        .header("Referer", destination)
        .send()
        .await
        .expect("Failed to send request")
}
