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

    let mut services = linkup_status(&state);
    let service_statuses = service_status(&state)?;
    services.extend(service_statuses);

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
        println!("{}", serde_json::to_string_pretty(&status).unwrap());
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
        println!();
        println!("Some linkup services are not running correctly. Please check the status of the services.");
        std::process::exit(1);
    }

    Ok(())
}

fn linkup_status(state: &LocalState) -> Vec<ServiceStatus> {
    let mut linkup_statuses: Vec<ServiceStatus> = Vec::new();
    let local_url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);

    let (tx, rx) = std::sync::mpsc::channel();

    let local_tx = tx.clone();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "local_server".to_string(),
            component_kind: "linkup".to_string(),
            location: local_url.clone(),
            status: server_status(local_url),
        };

        local_tx.send(service_status).unwrap();
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

        remote_tx.send(service_status).unwrap();
    });

    let tunnel_tx = tx.clone();
    let tunnel = state.linkup.tunnel.to_string();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "tunnel".to_string(),
            component_kind: "linkup".to_string(),
            location: tunnel.clone(),
            status: server_status(tunnel),
        };

        tunnel_tx.send(service_status).unwrap();
    });

    drop(tx);

    while let Ok(service_status) = rx.recv() {
        linkup_statuses.push(service_status);
    }

    linkup_statuses
}

fn service_status(state: &LocalState) -> Result<Vec<ServiceStatus>, CliError> {
    let mut service_statuses: Vec<ServiceStatus> = Vec::new();
    let (tx, rx) = std::sync::mpsc::channel();

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

            tx.send(service_status).unwrap();
        });
    }

    drop(tx);

    while let Ok(s) = rx.recv() {
        service_statuses.push(s);
    }

    Ok(service_statuses)
}

fn server_status(url: String) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let response = client.get(url).send();

    response.into()
}
