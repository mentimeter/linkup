use colored::{ColoredString, Colorize};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
            ServerStatus::Ok => "ok".green(),
            ServerStatus::Error => "error".red(),
            ServerStatus::Timeout => "timeout".yellow(),
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
    linkup_statuses.push(ServiceStatus {
        name: "local_server".to_string(),
        component_kind: "linkup".to_string(),
        location: local_url.to_string(),
        status: server_status(local_url),
    });

    // linkup_statuses.append(local_status);

    linkup_statuses.push(ServiceStatus {
        name: "remote_server".to_string(),
        component_kind: "linkup".to_string(),
        location: state.linkup.remote.to_string(),
        status: server_status(state.linkup.remote.to_string()),
    });

    linkup_statuses.push(ServiceStatus {
        name: "tunnel".to_string(),
        component_kind: "linkup".to_string(),
        location: state.linkup.tunnel.to_string(),
        status: server_status(state.linkup.tunnel.to_string()),
    });

    linkup_statuses
}

fn service_status(state: &LocalState) -> Result<Vec<ServiceStatus>, CliError> {
    let mut service_statuses: Vec<ServiceStatus> = Vec::new();

    for service in state.services.iter().cloned() {
        let url = match service.current {
            ServiceTarget::Local => service.local.clone(),
            ServiceTarget::Remote => service.remote.clone(),
        };

        let status = server_status(url.to_string());

        service_statuses.push(ServiceStatus {
            name: service.name,
            location: url.to_string(),
            component_kind: service.current.to_string(),
            status,
        });
    }

    Ok(service_statuses)
}

fn server_status(url: String) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();

    let response = client.get(url).send();

    match response {
        Ok(res) if res.status().is_server_error() => ServerStatus::Error,
        Ok(_) => ServerStatus::Ok,
        Err(_) => ServerStatus::Timeout,
    }
}
