use colored::{ColoredString, Colorize};
use linkup::{get_additional_headers, HeaderMap, StorableDomain, TargetService};
use serde::{Deserialize, Serialize};
use std::{thread, time::Duration};

use crate::{
    local_config::{LocalService, LocalState, ServiceTarget},
    CliError, LINKUP_LOCALSERVER_PORT,
};

#[derive(Deserialize, Serialize)]
struct Status {
    session: SessionStatus,
    services: Vec<ServiceStatus>,
}

#[derive(Deserialize, Serialize)]
pub struct SessionStatus {
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct ServiceStatus {
    name: String,
    status: ServerStatus,
    component_kind: String,
    location: String,
}

#[derive(Deserialize, Serialize, PartialEq)]
pub enum ServerStatus {
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

pub fn status(json: bool, all: bool) -> Result<(), CliError> {
    let state = LocalState::load()?;

    let (tx, rx) = std::sync::mpsc::channel();
    linkup_status(tx.clone(), &state);
    services_status(tx.clone(), &state);

    drop(tx);

    let mut services = rx.iter().collect::<Vec<ServiceStatus>>();
    services.sort_by(|a, b| {
        a.component_kind
            .cmp(&b.component_kind)
            .then(a.name.cmp(&b.name))
    });

    let mut status = Status {
        session: SessionStatus {
            name: state.linkup.session_name.clone(),
            domains: format_state_domains(&state.linkup.session_name, &state.domains),
        },
        services,
    };

    if !all && !json {
        status
            .services
            .retain(|s| s.status != ServerStatus::Ok || s.component_kind == "local");
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).expect("Failed to serialize status")
        );
    } else {
        print_session_status(status.session);

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
        if !json {
            println!("{}", "Some linkup services are not running correctly. Please check the status of the services.".yellow());
        }
        std::process::exit(1);
    }

    Ok(())
}

pub fn print_session_names(state: &LocalState) {
    print_session_status(SessionStatus {
        name: state.linkup.session_name.clone(),
        domains: format_state_domains(&state.linkup.session_name, &state.domains),
    });
}

pub fn format_state_domains(session_name: &str, domains: &[StorableDomain]) -> Vec<String> {
    // Filter out domains that are subdomains of other domains
    let filtered_domains = domains
        .iter()
        .filter(|&d| {
            !domains
                .iter()
                .any(|other| other.domain != d.domain && d.domain.ends_with(&other.domain))
        })
        .map(|d| d.domain.clone())
        .collect::<Vec<String>>();

    return filtered_domains
        .iter()
        .map(|domain| format!("https://{}.{}", session_name, domain.clone()))
        .collect();
}

pub fn print_session_status(session: SessionStatus) {
    // Display session information
    println!("Session Information:");
    println!("  Session Name: {}", session.name);
    println!("  Domains: ");
    for domain in session.domains {
        println!("    {}", domain);
    }
    println!();
}

fn linkup_status(tx: std::sync::mpsc::Sender<ServiceStatus>, state: &LocalState) {
    let local_url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);

    let local_tx = tx.clone();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "local_server".to_string(),
            component_kind: "linkup".to_string(),
            location: local_url.clone(),
            status: server_status(local_url, None),
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
            status: server_status(remote, None),
        };

        remote_tx
            .send(service_status)
            .expect("Failed to send linkup remote server status");
    });

    // NOTE(augustoccesar): last usage of tx on this context, no need to clone it
    let tunnel_tx = tx;
    let tunnel = state.get_tunnel_url().to_string();
    thread::spawn(move || {
        let service_status = ServiceStatus {
            name: "tunnel".to_string(),
            component_kind: "linkup".to_string(),
            location: tunnel.clone(),
            status: server_status(tunnel, None),
        };

        tunnel_tx
            .send(service_status)
            .expect("Failed to send linkup tunnel status");
    });
}

fn services_status(tx: std::sync::mpsc::Sender<ServiceStatus>, state: &LocalState) {
    for service in state.services.iter().cloned() {
        let tx = tx.clone();
        let session_name = state.linkup.session_name.clone();

        thread::spawn(move || {
            let url = match service.current {
                ServiceTarget::Local => service.local.clone(),
                ServiceTarget::Remote => service.remote.clone(),
            };

            let service_status = ServiceStatus {
                name: service.name.clone(),
                location: url.to_string(),
                component_kind: service.current.to_string(),
                status: service_status(&service, &session_name),
            };

            tx.send(service_status)
                .expect("Failed to send service status");
        });
    }
}

fn service_status(service: &LocalService, session_name: &str) -> ServerStatus {
    let url = match service.current {
        ServiceTarget::Local => service.local.clone(),
        ServiceTarget::Remote => service.remote.clone(),
    };

    let headers = get_additional_headers(
        url.as_ref(),
        &HeaderMap::new(),
        session_name,
        &TargetService {
            name: service.name.clone(),
            url: url.to_string(),
        },
    );

    server_status(url.to_string(), Some(headers))
}

pub fn server_status(url: String, extra_headers: Option<HeaderMap>) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();

    match client {
        Ok(client) => {
            let mut request = client.get(url);

            if let Some(extra_headers) = extra_headers {
                request = request.headers(extra_headers.into());
            }

            request.send().into()
        }
        Err(_) => ServerStatus::Error,
    }
}
