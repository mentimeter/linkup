use std::{
    env,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use linkup::{StorableDomain, StorableService, UpdateSessionRequest};
use linkup_local_server::linkup_router;
use reqwest::Url;
use tokio::{net::TcpListener, sync::OnceCell};

use anyhow::Result;

static INIT: OnceCell<()> = OnceCell::const_new();

#[derive(Debug)]
pub enum ServerKind {
    Local,
    Worker,
}

pub async fn setup_server(kind: ServerKind) -> String {
    println!("Setting up server of kind {:?}", kind);
    // Run command once
    match kind {
        ServerKind::Local => {
            let app = linkup_router();

            // Bind to a random port assigned by the OS
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });

            format!("http://{}", addr)
        }
        ServerKind::Worker => {
            INIT.get_or_init(|| async {
                boot_worker().expect("Failed to boot worker");
                // let _ = Command::new("echo")
                //     .arg("wrangler@latest")
                //     .arg("dev")
                //     .spawn()
                //     .expect("Failed to start wrangler dev command");
            })
            .await;
            wait_worker_started();
            format!("http://localhost:8787")
        }
    }
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

pub fn wait_worker_started() -> Result<()> {
    let mut count = 0;

    loop {
        let output = Command::new("bash")
            .arg("-c")
            .arg("lsof -i tcp:8787")
            .output()
            .expect("Failed to execute command");

        if output.status.success() {
            println!("Worker started.");
            break;
        } else if count == 20 {
            return Err(anyhow::anyhow!("Command failed after 20 retries"));
        } else {
            count += 1;
            thread::sleep(Duration::from_millis(500));
        }
    }

    Ok(())
}

pub fn boot_worker() -> Result<Child> {
    let original_cwd = env::current_dir()?;
    env::set_current_dir(Path::new("../worker"))?;

    Command::new("npm")
        .arg("install")
        .arg("-g")
        .arg("wrangler@latest")
        .status()?;

    let cmd = Command::new("npx")
        .arg("wrangler@latest")
        .arg("dev")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // .stdout(Stdio::inherit())
        // DEBUG POINT, use inherit stderr to see wrangler output
        .stderr(Stdio::null())
        // .stderr(Stdio::inherit())
        .spawn()?;

    thread::sleep(Duration::from_secs(5));

    env::set_current_dir(original_cwd)?;

    Ok(cmd)
}
