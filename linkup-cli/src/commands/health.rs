use std::{
    env,
    fmt::Display,
    fs::{self},
};

use clap::crate_version;
use colored::Colorize;
use regex::Regex;
use serde::Serialize;

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
    json: bool,
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

#[derive(Debug, Serialize)]
struct Session {
    name: String,
    tunnel_url: String,
}

impl Session {
    fn load(state: &LocalState) -> Self {
        Self {
            name: state.linkup.session_name.clone(),
            tunnel_url: state
                .linkup
                .tunnel
                .clone()
                .map(|url| url.as_str().to_string())
                .unwrap_or("None".to_string()),
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
enum BackgroundServiceHealth {
    NotInstalled,
    Stopped,
    Running(u32),
}

impl BackgroudServices {
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn load(state: &LocalState) -> Self {
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
            None => {
                if local_dns::is_installed(&crate::local_config::managed_domains(
                    Some(state),
                    &None,
                )) {
                    BackgroundServiceHealth::Stopped
                } else {
                    BackgroundServiceHealth::NotInstalled
                }
            }
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
    let env_var_format = Regex::new(r"[A-Z_][A-Z0-9_]*=.*").unwrap();

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
                    .replace(linkup_dir_path().to_str().unwrap(), "")
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
}

#[cfg(target_os = "macos")]
#[derive(Debug, Serialize)]
struct LocalDNS {
    is_installed: bool,
    resolvers: Vec<String>,
}

#[cfg(target_os = "macos")]
impl LocalDNS {
    fn load(state: &LocalState) -> Result<Self> {
        Ok(Self {
            is_installed: local_dns::is_installed(&crate::local_config::managed_domains(
                Some(state),
                &None,
            )),
            resolvers: local_dns::list_resolvers()?,
        })
    }
}

#[derive(Debug, Serialize)]
struct Health {
    system: System,
    session: Session,
    background_services: BackgroudServices,
    linkup: Linkup,
    #[cfg(target_os = "macos")]
    local_dns: LocalDNS,
}

impl Health {
    pub fn load() -> Result<Self> {
        let state = LocalState::load()?;
        let session = Session::load(&state);

        Ok(Self {
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
        writeln!(f, "{}", "System info:".bold().italic())?;
        writeln!(
            f,
            "  OS: {} ({})",
            self.system.os_name, self.system.os_version,
        )?;
        writeln!(f, "  Architecture: {}", self.system.arch)?;

        writeln!(f, "{}", "Session info:".bold().italic())?;
        writeln!(f, "  Name:       {}", self.session.name.normal())?;
        writeln!(f, "  Tunnel URL: {}", self.session.tunnel_url.normal())?;

        writeln!(f, "{}", "Background sevices:".bold().italic())?;
        write!(f, "  - Linkup Server  ")?;
        match &self.background_services.linkup_server {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
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
            }
        }

        write!(f, "  - Cloudflared    ")?;
        match &self.background_services.cloudflared {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
        }

        writeln!(f, "{}", "Linkup:".bold().italic())?;
        writeln!(f, "  Version: {}", self.linkup.version)?;
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
            writeln!(f, "{}", "Local DNS:".bold().italic())?;
            write!(f, "  Installed: ",)?;
            if self.local_dns.is_installed {
                writeln!(f, "{}", "YES".green())?;
            } else {
                writeln!(f, "{}", "NO".yellow())?;
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
