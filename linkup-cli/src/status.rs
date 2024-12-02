use colored::{ColoredString, Colorize};
use crossterm::{cursor, execute, style::Print, ExecutableCommand};
use linkup::{get_additional_headers, HeaderMap, StorableDomain, TargetService};
use serde::{Deserialize, Serialize};
use std::{
    io::stdout,
    ops::Deref,
    sync::mpsc::Receiver,
    thread::{self, sleep},
    time::Duration,
};
use url::Url;

use crate::{
    local_config::{LocalService, LocalState, ServiceTarget},
    CliError, LINKUP_LOCALSERVER_PORT,
};

const LOADING_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Status {
    session: SessionStatus,
    services: Vec<ServiceStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionStatus {
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ServiceStatus {
    name: String,
    status: ServerStatus,
    component_kind: String,
    location: String,
    service: LocalService,
    priority: i8,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum ServerStatus {
    Ok,
    Error,
    Timeout,
    Loading,
}

impl ServerStatus {
    fn colored(&self) -> ColoredString {
        match self {
            ServerStatus::Ok => "ok".blue(),
            ServerStatus::Error => "error".yellow(),
            ServerStatus::Timeout => "timeout".yellow(),
            ServerStatus::Loading => "loading".normal(),
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
    let state = LocalState::load()?;
    let linkup_services = linkup_services(&state);
    let all_services = state.services.into_iter().chain(linkup_services);

    let (services_statuses, status_receiver) =
        prepare_services_statuses(&state.linkup.session_name, all_services);

    let mut status = Status {
        session: SessionStatus {
            name: state.linkup.session_name.clone(),
            domains: format_state_domains(&state.linkup.session_name, &state.domains),
        },
        services: services_statuses,
    };

    status.services.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.component_kind.cmp(&b.component_kind))
            .then(a.name.cmp(&b.name))
    });

    if json {
        status_receiver.iter().for_each(|(name, server_status)| {
            for service_status in status.services.iter_mut() {
                if service_status.name == name {
                    service_status.status = server_status.clone();
                }
            }
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&status).expect("Failed to serialize status")
        );
    } else {
        print_session_status(&status.session);
        println!(
            "{:<20} {:<15} {:<8} LOCATION",
            "SERVICE NAME", "COMPONENT KIND", "STATUS",
        );

        stdout().execute(cursor::Hide).unwrap();
        ctrlc::set_handler(move || {
            stdout().execute(cursor::Show).unwrap();
            std::process::exit(130);
        })
        .expect("Failed to set CTRL+C handler");

        let mut iteration = 0;
        let mut loading_char_iteration = 0;
        let mut updated_services = 0;
        loop {
            while let Some((name, server_status)) = status_receiver.try_iter().next() {
                for service_status in status.services.iter_mut() {
                    if service_status.name == name {
                        service_status.status = server_status.clone();
                        updated_services += 1;
                    }
                }
            }

            // It has to print the services statuses at least once before we can move the cursor
            // to the start of the stuses section.
            if iteration > 0 {
                execute!(stdout(), cursor::MoveUp(status.services.len() as u16)).unwrap();
            }

            for i in 0..status.services.len() {
                let status = &status.services[i];

                let display_status = match &status.status {
                    ServerStatus::Loading => {
                        LOADING_CHARS[loading_char_iteration].to_string().normal()
                    }
                    status => status.colored(),
                };

                let mut status_name = ColoredString::from(status.name.clone());
                let mut status_component_kind = ColoredString::from(status.component_kind.clone());
                let mut status_location = ColoredString::from(status.location.clone());

                if status_component_kind.deref() == "local" {
                    status_name = status_name.bright_magenta();
                    status_component_kind = status_component_kind.bright_magenta();
                    status_location = status_location.bright_magenta();
                };

                let display_status = &format!(
                    "{:<20} {:<15} {:<8} {}\n",
                    status_name, status_component_kind, display_status, status_location
                );

                execute!(stdout(), Print(display_status),).unwrap();
            }

            if updated_services == status.services.len() {
                break;
            }

            loading_char_iteration = (iteration + 1) % LOADING_CHARS.len();
            iteration += 1;

            sleep(Duration::from_millis(50));
        }

        stdout().execute(cursor::Show).unwrap();
    }

    Ok(())
}

pub fn print_session_names(state: &LocalState) {
    print_session_status(&SessionStatus {
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

pub fn print_session_status(session: &SessionStatus) {
    println!("Session Name: {}", session.name);
    println!("Domains: ");
    for domain in &session.domains {
        println!("    {}", domain);
    }
    println!();
}

fn linkup_services(state: &LocalState) -> Vec<LocalService> {
    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT)).unwrap();

    vec![
        LocalService {
            name: "linkup_local_server".to_string(),
            remote: local_url.clone(),
            local: local_url.clone(),
            current: ServiceTarget::Local,
            directory: None,
            rewrites: vec![],
        },
        LocalService {
            name: "linkup_remote_server".to_string(),
            remote: state.linkup.remote.clone(),
            local: state.linkup.remote.clone(),
            current: ServiceTarget::Remote,
            directory: None,
            rewrites: vec![],
        },
        LocalService {
            name: "tunnel".to_string(),
            remote: state.get_tunnel_url(),
            local: state.get_tunnel_url(),
            current: ServiceTarget::Remote,
            directory: None,
            rewrites: vec![],
        },
    ]
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

fn prepare_services_statuses<I>(
    session_name: &str,
    services: I,
) -> (Vec<ServiceStatus>, Receiver<(String, ServerStatus)>)
where
    I: Iterator<Item = LocalService> + Clone,
{
    let services_statuses: Vec<ServiceStatus> = services
        .clone()
        .map(|service| {
            let url = match service.current {
                ServiceTarget::Local => service.local.clone(),
                ServiceTarget::Remote => service.remote.clone(),
            };

            let priority = service_priority(&service);

            ServiceStatus {
                name: service.name.clone(),
                location: url.to_string(),
                component_kind: service.current.to_string(),
                status: ServerStatus::Loading,
                service,
                priority,
            }
        })
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();

    for service in services {
        let tx = tx.clone();
        let service_clone = service.clone();
        let session_name = session_name.to_string();

        thread::spawn(move || {
            let status = service_status(&service_clone, &session_name);

            tx.send((service_clone.name.clone(), status))
                .expect("Failed to send service status");
        });
    }

    drop(tx);

    (services_statuses, rx)
}

fn is_internal_service(service: &LocalService) -> bool {
    service.name == "linkup_local_server"
        || service.name == "linkup_remote_server"
        || service.name == "tunnel"
}

fn service_priority(service: &LocalService) -> i8 {
    if is_internal_service(service) {
        1
    } else {
        2
    }
}
