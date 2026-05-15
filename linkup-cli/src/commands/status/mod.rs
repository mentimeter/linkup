mod health;

use std::{collections::HashMap, path::PathBuf, sync::mpsc::Receiver, thread, time::Duration};

use anyhow::Context;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use linkup::{
    HeaderMap, SessionDetailResponse, TargetService,
    config::{Config, HealthConfig},
    get_additional_headers,
};
use linkup_clients::LocalServerClient;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    config::load_config,
    services::{self, local_server},
    session::{SessionRow, list_session_rows, print_sessions_table},
    state::State,
};

pub use health::{ServerStatus, server_status};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub json: bool,

    #[arg(
        long,
        value_name = "NAME",
        help = "Session to inspect (defaults to the tunneled session)"
    )]
    pub session: Option<String>,
}

pub async fn status(args: &Args) -> anyhow::Result<()> {
    if !local_server::is_reachable().await {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    let state = State::load().context("Failed to load local state")?;

    let target_session = args
        .session
        .as_deref()
        .unwrap_or(&state.linkup.session_name)
        .to_string();

    let all_sessions = list_session_rows().await;

    if args.session.is_some()
        && !all_sessions
            .iter()
            .any(|session| session.name == target_session)
    {
        return Err(anyhow::anyhow!(
            "Session '{}' not found on local server",
            target_session
        ));
    }

    let session_detail = fetch_session_detail(&target_session).await;

    let config_path: PathBuf = state
        .linkup
        .config_path
        .parse()
        .expect("Config path stored on state should be valid Path");

    let config = load_config(&config_path).ok();

    let user_services = build_user_services(session_detail.as_ref(), config.as_ref());
    let internal_services = build_internal_services(&state);
    let all_services: Vec<ServiceToCheck> =
        user_services.into_iter().chain(internal_services).collect();

    let (mut service_statuses, status_receiver) =
        prepare_service_statuses(&target_session, all_services);

    service_statuses.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then(a.component_kind.cmp(&b.component_kind))
            .then(a.name.cmp(&b.name))
    });

    let mut output = Output {
        session_name: target_session.clone(),
        sessions: all_sessions.clone(),
        services: service_statuses,
    };

    if args.json {
        status_receiver.iter().for_each(|(name, server_status)| {
            for service in output.services.iter_mut() {
                if service.name == name {
                    service.status = server_status.clone();
                }
            }
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("Failed to serialize status")
        );
    } else {
        print_sessions_table(&all_sessions, Some(&target_session));
        println!();

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

        let mut progress_bars: HashMap<String, ProgressBar> = HashMap::new();

        let in_progress_style = ProgressStyle::with_template("{prefix} {spinner:<8.white} {msg:!}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");

        let done_style = ProgressStyle::with_template("{prefix} {msg}").unwrap();

        for service in &output.services {
            let progress_bar = multi_progress.add(ProgressBar::new_spinner());
            progress_bar.set_style(in_progress_style.clone());
            progress_bar.set_prefix(format!(
                "{:<22} {:<16}",
                service.name, service.component_kind
            ));
            progress_bar.set_message(service.url.to_string());
            progress_bar.enable_steady_tick(Duration::from_millis(50));

            progress_bars.insert(service.name.clone(), progress_bar);
        }

        let mut updated_count = 0;

        for (name, server_status) in status_receiver.iter() {
            for service in output.services.iter_mut() {
                if service.name == name {
                    service.status = server_status.clone();

                    if let Some(progress_bar) = progress_bars.get(&name) {
                        let status_text = format!("{:<8}", server_status.colored());
                        progress_bar.set_style(done_style.clone());
                        progress_bar
                            .finish_with_message(format!("{} {}", status_text, service.url));
                    }

                    updated_count += 1;
                }
            }

            if updated_count == output.services.len() {
                break;
            }
        }
    }

    Ok(())
}

struct ServiceToCheck {
    name: String,
    url: Url,
    component_kind: String,
    health: Option<HealthConfig>,
    priority: i8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ServiceStatus {
    name: String,
    status: ServerStatus,
    component_kind: String,
    url: Url,
    priority: i8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Output {
    session_name: String,
    sessions: Vec<SessionRow>,
    services: Vec<ServiceStatus>,
}

async fn fetch_session_detail(session_name: &str) -> Option<SessionDetailResponse> {
    let client = LocalServerClient::new(&services::local_server::url());
    client.get_session(session_name).await.ok()
}

fn build_user_services(
    session_detail: Option<&SessionDetailResponse>,
    config: Option<&Config>,
) -> Vec<ServiceToCheck> {
    let Some(session_detail) = session_detail else {
        return vec![];
    };

    session_detail
        .services
        .iter()
        .map(|session_service| {
            let config_service = config.and_then(|config| {
                config
                    .services
                    .iter()
                    .find(|config_service| config_service.name == session_service.name)
            });

            let component_kind = match config_service {
                Some(config_service) if config_service.local == config_service.remote => {
                    // TODO(@augustoccesar)[2026-04-29]: Think if this is enough of a check for us.
                    let host = config_service
                        .local
                        .host()
                        .expect("Host should exist on service URL")
                        .to_string();

                    if host.contains("localhost")
                        || host.contains("127.0.0.1")
                        || host.contains("0.0.0.0")
                    {
                        "local"
                    } else {
                        "remote"
                    }
                }
                Some(config_service) if session_service.location == config_service.remote => {
                    "remote"
                }
                Some(config_service) if session_service.location == config_service.local => "local",
                _ => "remote",
            }
            .to_string();

            ServiceToCheck {
                name: session_service.name.clone(),
                url: session_service.location.clone(),
                component_kind,
                health: config_service.and_then(|config_service| config_service.health.clone()),
                priority: 2,
            }
        })
        .collect()
}

fn build_internal_services(state: &State) -> Vec<ServiceToCheck> {
    let local_url = services::local_server::url();

    vec![
        ServiceToCheck {
            name: "linkup_local_server".to_string(),
            url: local_url.join("/linkup/check").unwrap(),
            component_kind: "local".to_string(),
            health: None,
            priority: 1,
        },
        ServiceToCheck {
            name: "linkup_remote_server".to_string(),
            url: state
                .linkup
                .worker_url
                .join("/linkup/check")
                .unwrap_or_else(|_| state.linkup.worker_url.clone()),
            component_kind: "remote".to_string(),
            health: None,
            priority: 1,
        },
        ServiceToCheck {
            name: "tunnel".to_string(),
            url: state
                .get_tunnel_url()
                .join("/linkup/check")
                .unwrap_or_else(|_| state.get_tunnel_url()),
            component_kind: "remote".to_string(),
            health: None,
            priority: 1,
        },
    ]
}

fn prepare_service_statuses(
    session_name: &str,
    services: Vec<ServiceToCheck>,
) -> (Vec<ServiceStatus>, Receiver<(String, ServerStatus)>) {
    let statuses: Vec<ServiceStatus> = services
        .iter()
        .map(|service| ServiceStatus {
            name: service.name.clone(),
            status: ServerStatus::Loading,
            component_kind: service.component_kind.clone(),
            url: service.url.clone(),
            priority: service.priority,
        })
        .collect();

    let (sender, receiver) = std::sync::mpsc::channel();

    for service in services {
        let sender = sender.clone();
        let session_name = session_name.to_string();

        thread::spawn(move || {
            let result = check_service(&service, &session_name);
            sender
                .send((service.name, result))
                .expect("Failed to send service status");
        });
    }

    drop(sender);

    (statuses, receiver)
}

fn check_service(service: &ServiceToCheck, session_name: &str) -> ServerStatus {
    let mut url = service.url.clone();
    let mut acceptable_statuses: Option<Vec<u16>> = None;

    if let Some(health) = &service.health {
        if let Some(path) = &health.path
            && let Ok(url_with_path) = url.join(path)
        {
            url = url_with_path;
        }

        if let Some(statuses) = &health.statuses {
            acceptable_statuses = Some(statuses.clone());
        }
    }

    let headers = get_additional_headers(
        url.as_ref(),
        &HeaderMap::new(),
        session_name,
        &TargetService {
            name: service.name.clone(),
            url: url.to_string(),
        },
    );

    server_status(url.as_str(), acceptable_statuses.as_ref(), Some(headers))
}
