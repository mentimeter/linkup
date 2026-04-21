use anyhow::Context;
use colored::{ColoredString, Colorize};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use linkup::{Domain, HeaderMap, TargetService, config::HealthConfig, get_additional_headers};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::mpsc::Receiver, thread, time::Duration};

use crate::{
    commands, services,
    state::{LocalService, ServiceTarget, State},
};

#[derive(clap::Args)]
pub struct Args {
    // Output status in JSON format
    #[arg(long)]
    pub json: bool,
}

pub fn status(args: &Args) -> anyhow::Result<()> {
    if !State::exists() {
        println!(
            "{}",
            "Seems like you don't have any state yet, so there is no status to report.".yellow()
        );
        println!("{}", "Have you run 'linkup start' at least once?".yellow());

        return Ok(());
    }

    let state = State::load().context("Failed to load local state")?;

    let linkup_services = linkup_services(&state);
    let all_services = state.clone().services.into_iter().chain(linkup_services);

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

    if args.json {
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
        status.session.print();
        println!();

        match commands::health::BackgroundServices::load(Some(&state)).linkup_server {
            commands::health::BackgroundServiceHealth::Running(_) => (),
            _ => println!("{}", "Linkup is not currently running.\n".yellow()),
        }

        let multi_progress = MultiProgress::new();

        multi_progress
            .println(format!(
                "{:<22} {:<16} {:<8} {}",
                "SERVICE NAME".bold(),
                "COMPONENT KIND".bold(),
                "STATUS".bold(),
                "LOCATION".bold(),
            ))
            .expect("printing should not fail");

        let mut services_progress_bars: HashMap<String, ProgressBar> = HashMap::new();

        let in_progress_style = ProgressStyle::with_template("{prefix} {spinner:<8.white} {msg:!}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");

        let done_style = ProgressStyle::with_template("{prefix} {msg}").unwrap();

        for service in &status.services {
            let progress_bar = multi_progress.add(ProgressBar::new_spinner());
            progress_bar.set_style(in_progress_style.clone());
            progress_bar.set_prefix(format!(
                "{:<22} {:<16}",
                service.name, service.component_kind
            ));
            progress_bar.set_message(service.service.current_url().to_string());
            progress_bar.enable_steady_tick(Duration::from_millis(50));

            services_progress_bars.insert(service.name.clone(), progress_bar);
        }

        let mut updated_services = 0;

        for (name, server_status) in status_receiver.iter() {
            for service_status in status.services.iter_mut() {
                if service_status.name == name {
                    service_status.status = server_status.clone();

                    if let Some(pb) = services_progress_bars.get(&name) {
                        let status_text = format!("{:<8}", server_status.colored());
                        let location = service_status.service.current_url().to_string();

                        pb.set_style(done_style.clone());
                        pb.finish_with_message(format!("{} {}", status_text, location));
                    }

                    updated_services += 1;
                }
            }

            if updated_services == status.services.len() {
                break;
            }
        }
    }

    Ok(())
}

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

impl SessionStatus {
    pub fn print(&self) {
        println!("Session Name: {}", self.name);
        println!("Domains: ");
        for domain in &self.domains {
            println!("    {}", domain);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ServiceStatus {
    name: String,
    status: ServerStatus,
    component_kind: String,
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

pub fn format_state_domains(session_name: &str, domains: &[Domain]) -> Vec<String> {
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

    filtered_domains
        .iter()
        .map(|domain| format!("https://{}.{}", session_name, domain.clone()))
        .collect()
}

fn linkup_services(state: &State) -> Vec<LocalService> {
    let local_url = services::local_server::url();

    vec![
        LocalService {
            current: ServiceTarget::Local,
            config: linkup::config::ServiceConfig {
                name: "linkup_local_server".to_string(),
                remote: local_url.clone(),
                local: local_url.clone(),
                directory: None,
                rewrites: None,
                health: Some(HealthConfig {
                    path: Some("/linkup/check".to_string()),
                    ..Default::default()
                }),
            },
        },
        LocalService {
            current: ServiceTarget::Remote,
            config: linkup::config::ServiceConfig {
                name: "linkup_remote_server".to_string(),
                remote: state.linkup.worker_url.clone(),
                local: state.linkup.worker_url.clone(),
                directory: None,
                rewrites: None,
                health: Some(HealthConfig {
                    path: Some("/linkup/check".to_string()),
                    ..Default::default()
                }),
            },
        },
        LocalService {
            current: ServiceTarget::Remote,
            config: linkup::config::ServiceConfig {
                name: "tunnel".to_string(),
                remote: state.get_tunnel_url(),
                local: state.get_tunnel_url(),
                directory: None,
                rewrites: None,
                health: Some(HealthConfig {
                    path: Some("/linkup/check".to_string()),
                    ..Default::default()
                }),
            },
        },
    ]
}

fn service_status(service: &LocalService, session_name: &str) -> ServerStatus {
    let mut acceptable_statuses_override: Option<Vec<u16>> = None;
    let mut url = service.current_url();

    if let Some(health_config) = &service.config.health {
        if let Some(path) = &health_config.path {
            url = url.join(path).unwrap();
        }

        if let Some(statuses) = &health_config.statuses {
            acceptable_statuses_override = Some(statuses.clone());
        }
    }

    let headers = get_additional_headers(
        url.as_ref(),
        &HeaderMap::new(),
        session_name,
        &TargetService {
            name: service.config.name.clone(),
            url: url.to_string(),
        },
    );

    server_status(
        url.as_str(),
        acceptable_statuses_override.as_ref(),
        Some(headers),
    )
}

pub fn server_status(
    url: &str,
    acceptable_statuses_override: Option<&Vec<u16>>,
    extra_headers: Option<HeaderMap>,
) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();

    match client {
        Ok(client) => {
            let mut req = client.get(url);

            if let Some(extra_headers) = extra_headers {
                req = req.headers(extra_headers.into());
            }

            match req.send() {
                Ok(res) => {
                    log::debug!(
                        "'{}' responded with status: {}. Acceptable statuses: {:?}",
                        url,
                        res.status().as_u16(),
                        acceptable_statuses_override
                    );

                    match (acceptable_statuses_override, res.status()) {
                        (None, status) => {
                            if !status.is_server_error() {
                                ServerStatus::Ok
                            } else {
                                ServerStatus::Error
                            }
                        }
                        (Some(override_statuses), status) => {
                            if override_statuses.contains(&status.as_u16()) {
                                ServerStatus::Ok
                            } else {
                                ServerStatus::Error
                            }
                        }
                    }
                }
                Err(_) => ServerStatus::Error,
            }
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
            let priority = service_priority(&service);

            ServiceStatus {
                name: service.config.name.clone(),
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

            tx.send((service_clone.config.name.clone(), status))
                .expect("Failed to send service status");
        });
    }

    drop(tx);

    (services_statuses, rx)
}

fn is_internal_service(service: &LocalService) -> bool {
    let service_name = &service.config.name;

    service_name == "linkup_local_server"
        || service_name == "linkup_remote_server"
        || service_name == "tunnel"
}

fn service_priority(service: &LocalService) -> i8 {
    if is_internal_service(service) { 1 } else { 2 }
}
