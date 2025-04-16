use clap::crate_version;
use colored::Colorize;
use regex::Regex;
use serde::Serialize;
use std::{
    env,
    fmt::Display,
    fs::{self},
};

use crate::{
    linkup_dir_path,
    local_config::LocalState,
    services::{self, find_service_pid, BackgroundService},
    Result,
};

#[cfg(target_os = "macos")]
use super::local_dns;

#[derive(clap::Args)]
pub struct Args {
    // Output status in JSON format
    #[arg(long)]
    pub json: bool,
}

pub fn health(args: &Args) -> Result<()> {
    let health = Health::load()?;

    let health = if args.json {
        serde_json::to_string_pretty(&health).unwrap()
    } else {
        format!("{}", health)
    };

    println!("{}", health);

    Ok(())
}

#[derive(Debug, Serialize)]
struct System {
    os_name: String,
    os_version: String,
    arch: String,
}

impl System {
    fn load() -> Self {
        Self {
            os_name: sysinfo::System::name().unwrap(),
            os_version: sysinfo::System::os_version().unwrap(),
            arch: env::consts::ARCH.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Default)]
struct Session {
    name: Option<String>,
    tunnel_url: Option<String>,
}

impl Session {
    fn load(state: &Option<LocalState>) -> Self {
        match state {
            Some(state) => Self {
                name: Some(state.linkup.session_name.clone()),
                tunnel_url: Some(
                    state
                        .linkup
                        .tunnel
                        .clone()
                        .map(|url| url.as_str().to_string())
                        .unwrap_or("None".to_string()),
                ),
            },
            None => Session::default(),
        }
    }
}

#[derive(Debug, Serialize)]
struct OrphanProcess {
    cmd: String,
    pid: u32,
}

#[derive(Debug, Serialize)]
struct BackgroudServices {
    linkup_server: BackgroundServiceHealth,
    cloudflared: BackgroundServiceHealth,
    #[cfg(target_os = "macos")]
    dns_server: BackgroundServiceHealth,
    possible_orphan_processes: Vec<OrphanProcess>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum BackgroundServiceHealth {
    Unknown,
    NotInstalled,
    Stopped,
    Running(u32),
}

impl BackgroudServices {
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn load(state: &Option<LocalState>) -> Self {
        let mut managed_pids: Vec<services::Pid> = Vec::with_capacity(4);

        let linkup_server = match find_service_pid(services::LocalServer::ID) {
            Some(pid) => {
                managed_pids.push(pid);

                BackgroundServiceHealth::Running(pid.as_u32())
            }
            None => BackgroundServiceHealth::Stopped,
        };

        let cloudflared = if services::is_cloudflared_installed() {
            match find_service_pid(services::CloudflareTunnel::ID) {
                Some(pid) => {
                    managed_pids.push(pid);

                    BackgroundServiceHealth::Running(pid.as_u32())
                }
                None => BackgroundServiceHealth::Stopped,
            }
        } else {
            BackgroundServiceHealth::NotInstalled
        };

        #[cfg(target_os = "macos")]
        let dns_server = match find_service_pid(services::LocalDnsServer::ID) {
            Some(pid) => {
                managed_pids.push(pid);

                BackgroundServiceHealth::Running(pid.as_u32())
            }
            None => match state {
                // If there is no state, we cannot know if local-dns is installed since we depend on
                // the domains listed on it.
                Some(state) => {
                    if local_dns::is_installed(&crate::local_config::managed_domains(
                        Some(state),
                        &None,
                    )) {
                        BackgroundServiceHealth::Stopped
                    } else {
                        BackgroundServiceHealth::NotInstalled
                    }
                }
                None => BackgroundServiceHealth::Unknown,
            },
        };

        Self {
            linkup_server,
            cloudflared,
            #[cfg(target_os = "macos")]
            dns_server,
            possible_orphan_processes: find_potential_orphan_processes(managed_pids),
        }
    }
}

fn find_potential_orphan_processes(managed_pids: Vec<services::Pid>) -> Vec<OrphanProcess> {
    let env_var_format = Regex::new(r"^[A-Z_][A-Z0-9_]*=.*$").unwrap();

    let current_pid = sysinfo::get_current_pid().unwrap();
    let mut orphans = Vec::new();

    for (pid, process) in services::system().processes() {
        if pid == &current_pid || managed_pids.contains(pid) {
            continue;
        }

        let command = process.cmd();
        for part in command.iter() {
            let mut part_string = part.to_string_lossy();

            if env_var_format.is_match(&part_string) {
                part_string = part_string
                    .replace(&linkup_dir_path().to_string_lossy().to_string(), "")
                    .into();
            }

            if part_string.contains("linkup") {
                let full_command = command
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");

                orphans.push(OrphanProcess {
                    cmd: truncate_with_ellipsis(&full_command, 120),
                    pid: pid.as_u32(),
                });
            }
        }
    }

    orphans
}

fn truncate_with_ellipsis(value: &str, max_len: usize) -> String {
    if value.len() > max_len {
        let mut truncated = value.chars().take(max_len - 3).collect::<String>();

        truncated.push_str("...");
        truncated
    } else {
        value.to_string()
    }
}

#[derive(Debug, Serialize)]
struct Linkup {
    version: String,
    config_location: String,
    config_exists: bool,
    config_content: Vec<String>,
}

impl Linkup {
    fn load() -> Result<Self> {
        let dir_path = linkup_dir_path();
        let files: Vec<String> = fs::read_dir(&dir_path)?
            .map(|f| f.unwrap().file_name().into_string().unwrap())
            .collect();

        Ok(Self {
            version: crate_version!().to_string(),
            config_location: dir_path.to_str().unwrap_or_default().to_string(),
            config_exists: dir_path.exists(),
            config_content: files,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn is_cap_set(&self) -> bool {
        let output = std::process::Command::new("getcap")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .stdin(std::process::Stdio::null())
            .args([crate::linkup_exe_path().unwrap().display().to_string()])
            .output();

        match output {
            Ok(output) => {
                if !output.status.success() {
                    let error_message = String::from_utf8_lossy(&output.stderr).to_string();
                    log::warn!("Failed to check capabilities: {}", error_message);
                    return false;
                }

                let output_text = String::from_utf8_lossy(&output.stdout).to_string();
                output_text.contains("cap_net_bind_service=ep")
            }
            Err(error) => {
                log::warn!("Failed to check capabilities: {}", error);
                return false;
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Serialize)]
struct LocalDNS {
    is_installed: Option<bool>,
    resolvers: Vec<String>,
}

#[cfg(target_os = "macos")]
impl LocalDNS {
    fn load(state: &Option<LocalState>) -> Result<Self> {
        // If there is no state, we cannot know if local-dns is installed since we depend on
        // the domains listed on it.
        let is_installed = state.as_ref().map(|state| {
            local_dns::is_installed(&crate::local_config::managed_domains(Some(state), &None))
        });

        Ok(Self {
            is_installed,
            resolvers: local_dns::list_resolvers()?,
        })
    }
}

#[derive(Debug, Serialize)]
struct Health {
    state_exists: bool,
    system: System,
    session: Session,
    background_services: BackgroudServices,
    linkup: Linkup,
    #[cfg(target_os = "macos")]
    local_dns: LocalDNS,
}

impl Health {
    pub fn load() -> Result<Self> {
        let state = LocalState::load().ok();
        let session = Session::load(&state);

        Ok(Self {
            state_exists: state.is_some(),
            system: System::load(),
            session,
            background_services: BackgroudServices::load(&state),
            linkup: Linkup::load()?,
            #[cfg(target_os = "macos")]
            local_dns: LocalDNS::load(&state)?,
        })
    }
}

impl Display for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.state_exists {
            writeln!(f, "\n{}", "It seems like you don't have a state file yet. This will cause some of the information to be UNKNOWN.".yellow())?;
            writeln!(
                f,
                "{}\n",
                "A state file is created after you run 'linkup start' once.".yellow()
            )?;
        }

        writeln!(f, "{}", "System info:".bold().italic())?;
        writeln!(
            f,
            "  OS: {} ({})",
            self.system.os_name, self.system.os_version,
        )?;
        writeln!(f, "  Architecture: {}", self.system.arch)?;

        writeln!(f, "{}", "Session info:".bold().italic())?;
        write!(f, "  Name:       ")?;
        match &self.session.name {
            Some(name) => writeln!(f, "{}", name.normal())?,
            None => writeln!(f, "{}", "NOT SET".yellow())?,
        }
        write!(f, "  Tunnel URL: ")?;
        match &self.session.tunnel_url {
            Some(tunnel_url) => writeln!(f, "{}", tunnel_url.normal())?,
            None => writeln!(f, "{}", "NOT SET".yellow())?,
        }

        writeln!(f, "{}", "Background sevices:".bold().italic())?;
        write!(f, "  - Linkup Server  ")?;
        match &self.background_services.linkup_server {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
            BackgroundServiceHealth::Unknown => writeln!(f, "{}", "UNKNOWN".yellow())?,
        }

        #[cfg(target_os = "macos")]
        {
            write!(f, "  - DNS Server     ")?;
            match &self.background_services.dns_server {
                BackgroundServiceHealth::NotInstalled => {
                    writeln!(f, "{}", "NOT INSTALLED".yellow())?
                }
                BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
                BackgroundServiceHealth::Running(pid) => {
                    writeln!(f, "{} ({})", "RUNNING".blue(), pid)?
                }
                BackgroundServiceHealth::Unknown => writeln!(f, "{}", "UNKNOWN".yellow())?,
            }
        }

        write!(f, "  - Cloudflared    ")?;
        match &self.background_services.cloudflared {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
            BackgroundServiceHealth::Unknown => writeln!(f, "{}", "UNKNOWN".yellow())?,
        }

        writeln!(f, "{}", "Linkup:".bold().italic())?;
        writeln!(f, "  Version: {}", self.linkup.version)?;
        #[cfg(target_os = "linux")]
        {
            write!(f, "  Capability set: ")?;
            if self.linkup.is_cap_set() {
                writeln!(f, "{}", "YES".blue())?;
            } else {
                writeln!(f, "{}", "NO".yellow())?;
            }
        }
        writeln!(
            f,
            "  Config folder location: {}",
            self.linkup.config_location
        )?;
        writeln!(f, "  Config folder exists: {}", self.linkup.config_exists)?;
        write!(f, "  Config folder contents:")?;
        if self.linkup.config_content.is_empty() {
            writeln!(f, " {}", "EMPTY".yellow())?;
        } else {
            writeln!(f)?;
            for file in &self.linkup.config_content {
                writeln!(f, "    - {}", file)?;
            }
        }

        #[cfg(target_os = "macos")]
        {
            write!(f, "{}", "Local DNS: ".bold().italic())?;
            match self.local_dns.is_installed {
                Some(installed) => {
                    write!(f, "\n  Installed: ",)?;
                    if installed {
                        writeln!(f, "{}", "YES".green())?;
                    } else {
                        writeln!(f, "{}", "NO".yellow())?
                    }

                    write!(f, "  Resolvers:")?;
                    if self.local_dns.resolvers.is_empty() {
                        writeln!(f, " {}", "EMPTY".yellow())?;
                    } else {
                        writeln!(f)?;
                        for file in &self.local_dns.resolvers {
                            writeln!(f, "      - {}", file)?;
                        }
                    }
                }
                None => writeln!(f, "{}", "UNKNOWN".yellow())?,
            }
        }

        write!(f, "{}", "Possible orphan processes:".bold().italic())?;
        if self
            .background_services
            .possible_orphan_processes
            .is_empty()
        {
            writeln!(f, " {}", "NONE".yellow())?;
        } else {
            writeln!(f)?;
            for orphan_process in &self.background_services.possible_orphan_processes {
                writeln!(f, "    - ({}): {}", orphan_process.pid, orphan_process.cmd)?;
            }
        }

        Ok(())
    }
}
