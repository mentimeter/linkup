use std::{
    env,
    fmt::Display,
    fs::{self},
};

use clap::crate_version;
use colored::Colorize;
use serde::Serialize;

use crate::{linkup_dir_path, local_config::LocalState, services, CliError};

#[derive(clap::Args)]
pub struct Args {
    // Output status in JSON format
    #[arg(long)]
    json: bool,
}

pub fn health(args: &Args) -> Result<(), CliError> {
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
    fn load() -> Result<Self, CliError> {
        let state = LocalState::load()?;

        Ok(Self {
            name: state.linkup.session_name,
            tunnel_url: state
                .linkup
                .tunnel
                .map(|url| url.as_str().to_string())
                .unwrap_or("None".to_string()),
        })
    }
}

#[derive(Debug, Serialize)]
struct EnvironmentVariables {
    cf_api_token: bool,
    cf_zone_id: bool,
    cf_account_id: bool,
}

impl EnvironmentVariables {
    fn load() -> Self {
        Self {
            cf_api_token: env::var("LINKUP_CF_API_TOKEN").is_ok(),
            cf_zone_id: env::var("LINKUP_CLOUDFLARE_ZONE_ID").is_ok(),
            cf_account_id: env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").is_ok(),
        }
    }
}

#[derive(Debug, Serialize)]
struct BackgroudServices {
    linkup_server: BackgroundServiceHealth,
    caddy: BackgroundServiceHealth,
    dnsmasq: BackgroundServiceHealth,
    cloudflared: BackgroundServiceHealth,
}

#[derive(Debug, Serialize)]
enum BackgroundServiceHealth {
    NotInstalled,
    Stopped,
    Running(String),
}

impl BackgroudServices {
    fn load() -> Self {
        let linkup_server = match services::LocalServer::new().running_pid() {
            Some(pid) => BackgroundServiceHealth::Running(pid),
            None => BackgroundServiceHealth::Stopped,
        };

        let dnsmasq = if services::is_dnsmasq_installed() {
            match services::Dnsmasq::new().running_pid() {
                Some(pid) => BackgroundServiceHealth::Running(pid),
                None => BackgroundServiceHealth::Stopped,
            }
        } else {
            BackgroundServiceHealth::NotInstalled
        };

        let caddy = if services::is_caddy_installed() {
            match services::Caddy::new().running_pid() {
                Some(pid) => BackgroundServiceHealth::Running(pid),
                None => BackgroundServiceHealth::Stopped,
            }
        } else {
            BackgroundServiceHealth::NotInstalled
        };

        let cloudflared = if services::is_cloudflared_installed() {
            match services::CloudflareTunnel::new().running_pid() {
                Some(pid) => BackgroundServiceHealth::Running(pid),
                None => BackgroundServiceHealth::Stopped,
            }
        } else {
            BackgroundServiceHealth::NotInstalled
        };

        Self {
            linkup_server,
            caddy,
            dnsmasq,
            cloudflared,
        }
    }
}

#[derive(Debug, Serialize)]
struct Linkup {
    version: String,
    config_location: String,
    config_content: Vec<String>,
}

impl Linkup {
    fn load() -> Result<Self, CliError> {
        let dir_path = linkup_dir_path();
        let files: Vec<String> = fs::read_dir(&dir_path)?
            .map(|f| f.unwrap().file_name().into_string().unwrap())
            .collect();

        Ok(Self {
            version: crate_version!().to_string(),
            config_location: dir_path.to_str().unwrap_or_default().to_string(),
            config_content: files,
        })
    }
}

#[derive(Debug, Serialize)]
struct LocalDNS {
    resolvers: Vec<String>,
}

impl LocalDNS {
    fn load() -> Result<Self, CliError> {
        Ok(Self {
            resolvers: fs::read_dir("/etc/resolver/")?
                .map(|f| f.unwrap().file_name().into_string().unwrap())
                .collect(),
        })
    }
}

#[derive(Debug, Serialize)]
struct Health {
    system: System,
    session: Session,
    environment_variables: EnvironmentVariables,
    background_services: BackgroudServices,
    linkup: Linkup,
    local_dns: LocalDNS,
}

impl Health {
    pub fn load() -> Result<Self, CliError> {
        Ok(Self {
            system: System::load(),
            session: Session::load()?,
            environment_variables: EnvironmentVariables::load(),
            background_services: BackgroudServices::load(),
            linkup: Linkup::load()?,
            local_dns: LocalDNS::load()?,
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
        writeln!(f, "  Name:       {}", self.session.name)?;
        writeln!(f, "  Tunnel URL: {}", self.session.tunnel_url)?;

        writeln!(f, "{}", "Background sevices:".bold().italic())?;
        write!(f, "  - Linkup Server  ")?;
        match &self.background_services.linkup_server {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
        }
        write!(f, "  - Caddy          ")?;
        match &self.background_services.caddy {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
        }
        write!(f, "  - dnsmasq        ")?;
        match &self.background_services.dnsmasq {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
        }
        write!(f, "  - Cloudflared    ")?;
        match &self.background_services.cloudflared {
            BackgroundServiceHealth::NotInstalled => writeln!(f, "{}", "NOT INSTALLED".yellow())?,
            BackgroundServiceHealth::Stopped => writeln!(f, "{}", "NOT RUNNING".yellow())?,
            BackgroundServiceHealth::Running(pid) => writeln!(f, "{} ({})", "RUNNING".blue(), pid)?,
        }

        writeln!(f, "{}", "Environment variables:".bold().italic())?;

        write!(f, "  - LINKUP_CF_API_TOKEN          ")?;
        if self.environment_variables.cf_api_token {
            writeln!(f, "{}", "OK".blue())?;
        } else {
            writeln!(f, "{}", "MISSING".yellow())?;
        }

        write!(f, "  - LINKUP_CLOUDFLARE_ZONE_ID    ")?;
        if self.environment_variables.cf_zone_id {
            writeln!(f, "{}", "OK".blue())?;
        } else {
            writeln!(f, "{}", "MISSING".yellow())?;
        }

        write!(f, "  - LINKUP_CLOUDFLARE_ACCOUNT_ID ")?;
        if self.environment_variables.cf_account_id {
            writeln!(f, "{}", "OK".blue())?;
        } else {
            writeln!(f, "{}", "MISSING".yellow())?;
        }

        writeln!(f, "{}", "Linkup:".bold().italic())?;
        writeln!(f, "  Version: {}", self.linkup.version)?;
        writeln!(f, "  Config location: {}", self.linkup.config_location)?;
        writeln!(f, "  Config contents:")?;
        for file in &self.linkup.config_content {
            writeln!(f, "    - {}", file)?;
        }

        writeln!(f, "{}", "Local DNS resolvers:".bold().italic())?;
        for file in &self.local_dns.resolvers {
            writeln!(f, "    - {}", file)?;
        }

        Ok(())
    }
}
