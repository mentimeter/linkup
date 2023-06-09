use colored::{ColoredString, Colorize};
use serde::{Deserialize, Serialize};
use std::{thread, time::Duration};

use crate::{
    local_config::{LocalState, ServiceTarget},
    start::get_state,
    CliError, LINKUP_LOCALSERVER_PORT,
};

#[derive(Deserialize, Serialize)]
struct Status {
    session: SessionStatus,
    services: Vec<ServiceStatus>,
}

#[derive(Deserialize, Serialize)]
struct SessionStatus {
    name: String,
    domains: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct ServiceStatus {
    name: String,
    status: ServerStatus,
    component_kind: String,
    location: String,
}

#[derive(Deserialize, Serialize, PartialEq)]
enum ServerStatus {
    Ok,
    Error,
    Timeout,
}

impl ServerStatus {
    fn colored(&self) -> ColoredString {
        match self {
            ServerStatus::Ok => "ok".blue(),
            ServerStatus::Error => "error".yellow(),
            ServerStatus::Timeout => "timeout".yellow(),
        }
    }
}

impl From<Result<reqwest::blocking::Response, reqwest::Error>> for ServerStatus {
    fn from(res: Result<reqwest::blocking::Response, reqwest::Error>) -> Self {
        match res {
            Ok(res) if res.status().is_server_error() => ServerStatus::Error,
            Ok(_) => ServerStatus::Ok,
            Err(_) => ServerStatus::Timeout,
        }
    }
}

pub fn status(json: bool) -> Result<(), CliError> {
    let state = get_state()?;

    let (tx, rx) = std::sync::mpsc::channel();
    linkup_status(tx.clone(), &state);
    service_status(tx.clone(), &state);

    drop(tx);

    let mut services = rx.iter().collect::<Vec<ServiceStatus>>();
    services.sort_by(|a, b| {
        a.component_kind
            .cmp(&b.component_kind)
            .then(a.name.cmp(&b.name))
    });

    // Filter out domains that are subdomains of other domains
    let filtered_domains = state
        .domains
        .iter()
        .filter(|&d| {
            !state
                .domains
                .iter()
                .any(|other| other.domain != d.domain && d.domain.ends_with(&other.domain))
        })
        .map(|d| d.domain.clone())
        .collect::<Vec<String>>();

    let status = Status {
        session: SessionStatus {
            name: state.linkup.session_name.clone(),
            domains: filtered_domains
                .iter()
                .map(|d| format!("{}.{}", state.linkup.session_name.clone(), d.clone()))
                .collect(),
        },
        services,
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).expect("Failed to serialize status")
        );
    } else {
        // Display session information
        println!("Session Information:");
        println!("  Session Name: {}", status.session.name);
        println!("  Domains: ");
        for domain in &status.session.domains {
            println!("    {}", domain);
        }
        println!();

        // Display services information
        println!("Service Information:");
        println!(
            "{:<15} {:<15} {:<15} {:<15}",
            "Service Name", "Component Kind", "Status", "Location"
        );
        for status in &status.services {
            println!(
                "{:<15} {:<15} {:<15} {:<15}",
                status.name,
                status.component_kind,
                status.status.colored(),
                status.location
            );
        }
        println!();
    }

    if status
        .services
        .iter()
        .any(|s| s.component_kind == "linkup" && s.status != ServerStatus::Ok)
    {
        println!("{}", "Some linkup services are not running correctly. Please check the status of the services.".yellow());
        std::process::exit(1);
    }

    Ok(())
}

fn linkup_status(tx: std::sync::mpsc::Sender<ServiceStatus>, state: &LocalState) {
    let local_url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);

    let local_tx = tx.clone();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "local_server".to_string(),
            component_kind: "linkup".to_string(),
            location: local_url.clone(),
            status: server_status(local_url),
        };

        local_tx
            .send(service_status)
            .expect("Failed to send linkup local server status")
    });

    let remote_tx = tx.clone();
    // TODO(augustoccesar): having to clone this remote on the ServiceStatus feels unnecessary. Look if it can be reference
    let remote = state.linkup.remote.to_string();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "remote_server".to_string(),
            component_kind: "linkup".to_string(),
            location: remote.clone(),
            status: server_status(remote),
        };

        remote_tx
            .send(service_status)
            .expect("Failed to send linkup remote server status");
    });

    // NOTE(augustoccesar): last usage of tx on this context, no need to clone it
    let tunnel_tx = tx;
    let tunnel = state.linkup.tunnel.to_string();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "tunnel".to_string(),
            component_kind: "linkup".to_string(),
            location: tunnel.clone(),
            status: server_status(tunnel),
        };

        tunnel_tx
            .send(service_status)
            .expect("Failed to send linkup tunnel status");
    });
}

fn service_status(tx: std::sync::mpsc::Sender<ServiceStatus>, state: &LocalState) {
    for service in state.services.iter().cloned() {
        let tx = tx.clone();

        thread::spawn(move || {
            let url = match service.current {
                ServiceTarget::Local => service.local.clone(),
                ServiceTarget::Remote => service.remote.clone(),
            };

            let service_status = ServiceStatus {
                name: service.name,
                location: url.to_string(),
                component_kind: service.current.to_string(),
                status: server_status(url.to_string()),
            };

            tx.send(service_status)
                .expect("Failed to send service status");
        });
    }
}

fn server_status(url: String) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();

    match client {
        Ok(client) => client.get(url).send().into(),
        Err(_) => ServerStatus::Error,
    }
}
