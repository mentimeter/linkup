use std::{path::PathBuf, process::Command};

use linkup::{Domain, MemoryStringStore, SessionAllocator, SessionService, UpsertSessionRequest};
use linkup_clients::WorkerClient;
use linkup_local_server::{ServerState, dns::DnsCatalog, router};
use reqwest::Url;
use tokio::net::TcpListener;

#[derive(Debug)]
pub enum ServerKind {
    Local,
    Worker,
}

pub async fn setup_server(kind: ServerKind) -> String {
    match kind {
        ServerKind::Local => {
            let state = ServerState {
                session_allocator: SessionAllocator::new(MemoryStringStore::default()),
                https_client: linkup_clients::https_client(),
                dns_catalog: DnsCatalog::new(),
                https_certs_dir: PathBuf::default(),
                worker_client: WorkerClient::new(
                    &Url::parse("http://localhost").unwrap(),
                    "token123",
                ),
            };

            let app = router(state);

            // Bind to a random port assigned by the OS
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });

            format!("http://{}", addr)
        }
        ServerKind::Worker => {
            if !check_worker_running() {
                panic!("Worker not running! Run npx wrangler@latest dev in the worker dir");
            }
            "http://localhost:8787".to_string()
        }
    }
}

pub async fn post(url: String, body: String) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer token123")
        // TODO(augustoccesar)[2025-02-24]: Proper test version header
        .header("x-linkup-version", "99.99.99")
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
    let req = UpsertSessionRequest::Named {
        desired_name: name,
        session_token: "token".to_string(),
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

pub fn check_worker_running() -> bool {
    let output = Command::new("bash")
        .arg("-c")
        .arg("lsof -i tcp:8787")
        .output()
        .expect("Failed to execute command");

    output.status.success()
}
