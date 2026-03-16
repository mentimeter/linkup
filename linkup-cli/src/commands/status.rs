use anyhow::Context;
use colored::{ColoredString, Colorize};
use crossterm::{cursor, execute, style::Print, terminal};
use linkup::{config::HealthConfig, get_additional_headers, Domain, HeaderMap, TargetService};
use serde::{Deserialize, Serialize};
use std::{
    io::stdout,
    ops::Deref,
    sync::mpsc::Receiver,
    thread::{self, sleep},
    time::Duration,
};

use crate::{
    commands, default_linkup_dir_path, linkup_dir_path, services,
    state::{LocalService, ServiceTarget, State},
    LINKUP_STATE_FILE,
};

const LOADING_CHARS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const MIN_WIDTH_FOR_LOCATION: usize = 110;
const MIN_WIDTH_FOR_KIND: usize = 50;

#[derive(clap::Args)]
pub struct Args {
    // Output status in JSON format
    #[arg(long)]
    pub json: bool,

    #[arg(short, long)]
    all: bool,
}

pub fn status(args: &Args) -> anyhow::Result<()> {
    // TODO(augustocesar)[2024-10-28]: Remove --all/-a in a future release.
    // Do not print the warning in case of JSON so it doesn't break any usage if the result of the command
    // is passed on to somewhere else.
    if args.all && !args.json {
        let warning = "--all/-a is a noop now. All services statuses will always be shown. \
            This arg will be removed in a future release.\n";
        println!("{}", warning.yellow());
    }

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
        print_instance_summary();
        println!();
        status.session.print();
        println!();

        match commands::health::BackgroundServices::load(Some(&state)).linkup_server {
            commands::health::BackgroundServiceHealth::Running(_) => (),
            _ => println!("{}", "Linkup is not currently running.\n".yellow()),
        }

        let mut stdout = stdout();

        execute!(stdout, cursor::Hide, terminal::DisableLineWrap)?;

        ctrlc::set_handler(move || {
            execute!(std::io::stdout(), cursor::Show, terminal::EnableLineWrap).unwrap();
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
                // +1 to include the header since it is also dynamic based on the width of the terminal.
                execute!(stdout, cursor::MoveUp((status.services.len() + 1) as u16))?;
            }

            let (terminal_width, _) = terminal::size().unwrap();

            execute!(
                stdout,
                terminal::Clear(terminal::ClearType::CurrentLine),
                Print(table_header(terminal_width))
            )?;

            for i in 0..status.services.len() {
                let status = &status.services[i];

                execute!(
                    stdout,
                    terminal::Clear(terminal::ClearType::CurrentLine),
                    Print(status.as_table_row(loading_char_iteration, terminal_width))
                )?;
            }

            if updated_services == status.services.len() {
                break;
            }

            loading_char_iteration = (iteration + 1) % LOADING_CHARS.len();
            iteration += 1;

            sleep(Duration::from_millis(50));
        }

        execute!(stdout, cursor::Show, terminal::EnableLineWrap).unwrap();
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

impl ServiceStatus {
    pub fn as_table_row(&self, loading_iter: usize, terminal_width: u16) -> String {
        let terminal_width = terminal_width as usize;

        let display_status = match &self.status {
            ServerStatus::Loading => LOADING_CHARS[loading_iter].to_string().normal(),
            status => status.colored(),
        };

        let mut status_name = ColoredString::from(self.name.clone());
        let mut status_component_kind = ColoredString::from(self.component_kind.clone());
        let mut status_location = ColoredString::from(self.service.current_url().to_string());

        if status_component_kind.deref() == "local" {
            status_name = status_name.bright_magenta();
            status_component_kind = status_component_kind.bright_magenta();
            status_location = status_location.bright_magenta();
        };

        let mut output = String::with_capacity(MIN_WIDTH_FOR_LOCATION);
        output.push_str(&format!("{:<22}", status_name));

        if terminal_width > MIN_WIDTH_FOR_KIND {
            output.push_str(&format!("{:<16}", status_component_kind));
        }

        output.push_str(&format!("{:<8}", display_status));

        if terminal_width > MIN_WIDTH_FOR_LOCATION {
            output.push_str(&status_location);
        }

        output.push('\n');

        output
    }
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

fn table_header(terminal_width: u16) -> String {
    let terminal_width = terminal_width as usize;

    let mut output = String::with_capacity(110);
    output.push_str(&format!("{:<22}", "SERVICE NAME"));

    if terminal_width > MIN_WIDTH_FOR_KIND {
        output.push_str(&format!("{:<16}", "COMPONENT KIND"));
    }

    output.push_str(&format!("{:<8}", "STATUS"));

    if terminal_width > MIN_WIDTH_FOR_LOCATION {
        output.push_str("LOCATION");
    }

    output.push('\n');

    output
}

pub fn primary_domains(domains: &[Domain]) -> Vec<&Domain> {
    domains
        .iter()
        .filter(|d| {
            !domains
                .iter()
                .any(|other| other.domain != d.domain && d.domain.ends_with(&other.domain))
        })
        .collect()
}

pub fn format_state_domains(session_name: &str, domains: &[Domain]) -> Vec<String> {
    primary_domains(domains)
        .iter()
        .map(|d| format!("https://{}.{}", session_name, d.domain))
        .collect()
}

fn linkup_services(state: &State) -> Vec<LocalService> {
    let port = state.linkup.local_server_port.unwrap_or(80);
    let local_url = services::LocalServer::url(port);

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

fn print_instance_summary() {
    let default_dir = default_linkup_dir_path();
    let current_dir = linkup_dir_path();
    let instances_dir = default_dir.join("instances");

    struct InstanceInfo {
        id: String,
        domain: String,
        path: String,
        is_current: bool,
    }

    fn instance_from_state(state: &State) -> (String, String) {
        let path = std::path::Path::new(&state.linkup.config_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let domain = if state.linkup.session_name.is_empty() {
            "(unassigned)".to_string()
        } else {
            primary_domains(&state.domains)
                .first()
                .map(|d| format!("{}.{}", state.linkup.session_name, d.domain))
                .unwrap_or_else(|| "(unassigned)".to_string())
        };
        (path, domain)
    }

    let mut instances: Vec<InstanceInfo> = Vec::new();

    let default_state_path = default_dir.join(LINKUP_STATE_FILE);
    if default_state_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&default_state_path) {
            if let Ok(state) = serde_yaml::from_str::<State>(&content) {
                let (path, domain) = instance_from_state(&state);
                instances.push(InstanceInfo {
                    id: "default".to_string(),
                    domain,
                    path,
                    is_current: current_dir == default_dir,
                });
            }
        }
    }

    if instances_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&instances_dir) {
            let mut numbered: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect();
            numbered.sort_by_key(|e| {
                e.file_name()
                    .to_string_lossy()
                    .parse::<u32>()
                    .unwrap_or(u32::MAX)
            });

            for entry in numbered {
                let state_path = entry.path().join(LINKUP_STATE_FILE);
                if state_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&state_path) {
                        if let Ok(state) = serde_yaml::from_str::<State>(&content) {
                            let id = entry.file_name().to_string_lossy().to_string();
                            let (path, domain) = instance_from_state(&state);
                            instances.push(InstanceInfo {
                                id,
                                domain,
                                path,
                                is_current: current_dir == entry.path(),
                            });
                        }
                    }
                }
            }
        }
    }

    if instances.len() <= 1 {
        return;
    }

    let id_w = instances
        .iter()
        .map(|i| i.id.len())
        .max()
        .unwrap_or(2)
        .max(2);
    let domain_w = instances
        .iter()
        .map(|i| i.domain.len())
        .max()
        .unwrap_or(6)
        .max(6);

    println!("Instances:");
    println!("     {:<id_w$}  {:<domain_w$}  PATH", "ID", "DOMAIN");
    for inst in &instances {
        let marker = if inst.is_current { "📌" } else { "  " };
        let domain_cell = if inst.domain == "(unassigned)" {
            format!("{:<domain_w$}", inst.domain,)
        } else {
            format!(
                "\x1b]8;;https://{}\x1b\\{}\x1b]8;;\x1b\\{}",
                inst.domain,
                inst.domain,
                " ".repeat(domain_w.saturating_sub(inst.domain.len()))
            )
        };
        println!(
            "  {} {:<id_w$}  {}  {}",
            marker, inst.id, domain_cell, inst.path,
        );
    }
}

fn is_internal_service(service: &LocalService) -> bool {
    let service_name = &service.config.name;

    service_name == "linkup_local_server"
        || service_name == "linkup_remote_server"
        || service_name == "tunnel"
}

fn service_priority(service: &LocalService) -> i8 {
    if is_internal_service(service) {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(name: &str) -> Domain {
        Domain {
            domain: name.to_string(),
            default_service: "web".to_string(),
            routes: None,
        }
    }

    #[test]
    fn test_primary_domains_filters_subdomains() {
        let domains = vec![domain("example.com"), domain("app.example.com")];
        let result = primary_domains(&domains);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].domain, "example.com");
    }

    #[test]
    fn test_primary_domains_single() {
        let domains = vec![domain("example.com")];
        let result = primary_domains(&domains);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].domain, "example.com");
    }

    #[test]
    fn test_primary_domains_empty() {
        let domains: Vec<Domain> = vec![];
        let result = primary_domains(&domains);
        assert!(result.is_empty());
    }

    #[test]
    fn test_primary_domains_unrelated() {
        let domains = vec![domain("foo.com"), domain("bar.com")];
        let result = primary_domains(&domains);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_format_state_domains() {
        let domains = vec![domain("example.com"), domain("app.example.com")];
        let result = format_state_domains("my-session", &domains);
        assert_eq!(result, vec!["https://my-session.example.com"]);
    }
}
