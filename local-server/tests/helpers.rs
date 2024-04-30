use linkup::{StorableDomain, StorableService, UpdateSessionRequest};
use linkup_local_server::linkup_router;
use reqwest::Url;
use tokio::net::TcpListener;

pub async fn setup_server() -> String {
    let app = linkup_router();

    // Bind to a random port assigned by the OS
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}", addr)
}

pub async fn post(url: String, body: String) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .expect("Failed to send request")
}

pub fn create_session_request(name: String, fe_location: Option<String>) -> String {
    let location = match fe_location {
        Some(location) => location,
        None => "http://example.com".to_string(),
    };
    let req = UpdateSessionRequest {
        desired_name: name,
        session_token: "token".to_string(),
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
