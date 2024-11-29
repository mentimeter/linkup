use std::{
    env,
    fmt::Display,
    fs::{self},
};

use clap::crate_version;
use colored::Colorize;
use serde::Serialize;

use crate::{linkup_dir_path, local_config::LocalState, CliError};

#[derive(Debug, Serialize)]
struct System {
    os_name: String,
    os_version: String,
}

impl System {
    fn load() -> Self {
        Self {
            os_name: sysinfo::System::name().unwrap(),
            os_version: sysinfo::System::os_version().unwrap(),
        }
    }
}

#[derive(Debug, Serialize)]
struct Session {
    name: String,
}

impl Session {
    fn load() -> Result<Self, CliError> {
        let state = LocalState::load()?;

        Ok(Self {
            name: state.linkup.session_name,
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
    caddy_pids: Vec<String>,
    dnsmasq_pids: Vec<String>,
    cloudflared_pids: Vec<String>,
}

impl BackgroudServices {
    fn load() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        let mut dnsmasq_pids: Vec<String> = vec![];
        let mut caddy_pids: Vec<String> = vec![];
        let mut cloudflared_pids: Vec<String> = vec![];

        for (pid, process) in sys.processes() {
            let process_name = process.name();

            if process_name == "dnsmasq" {
                dnsmasq_pids.push(pid.to_string());
            } else if process_name == "caddy" {
                caddy_pids.push(pid.to_string());
            } else if process_name == "cloudflared" {
                cloudflared_pids.push(pid.to_string());
            }
        }

        Self {
            caddy_pids,
            cloudflared_pids,
            dnsmasq_pids,
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
            .map(|f| f.unwrap().file_name().to_str().unwrap().to_string())
            .collect();

        Ok(Self {
            version: crate_version!().to_string(),
            config_location: dir_path.to_str().unwrap_or_default().to_string(),
            config_content: files,
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
}

impl Health {
    pub fn load() -> Result<Self, CliError> {
        Ok(Self {
            system: System::load(),
            session: Session::load()?,
            environment_variables: EnvironmentVariables::load(),
            background_services: BackgroudServices::load(),
            linkup: Linkup::load()?,
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

        writeln!(f, "{}", "Session info:".bold().italic())?;
        writeln!(f, "  Name: {}", self.session.name)?;

        writeln!(f, "{}", "Background sevices:".bold().italic())?;
        write!(f, "  - Caddy       ")?;
        if !self.background_services.caddy_pids.is_empty() {
            writeln!(
                f,
                "{} ({})",
                "RUNNING".blue(),
                self.background_services.caddy_pids.join(",")
            )?;
        } else {
            writeln!(f, "{}", "NOT RUNNING".yellow())?;
        }

        write!(f, "  - dnsmasq     ")?;
        if !self.background_services.dnsmasq_pids.is_empty() {
            writeln!(
                f,
                "{} ({})",
                "RUNNING".blue(),
                self.background_services.dnsmasq_pids.join(",")
            )?;
        } else {
            writeln!(f, "{}", "NOT RUNNING".yellow())?;
        }

        write!(f, "  - Cloudflared ")?;
        if !self.background_services.cloudflared_pids.is_empty() {
            writeln!(
                f,
                "{} ({})",
                "RUNNING".blue(),
                self.background_services.cloudflared_pids.join(",")
            )?;
        } else {
            writeln!(f, "{}", "NOT RUNNING".yellow())?;
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

        Ok(())
    }
}

pub fn health(json: bool) -> Result<(), CliError> {
    let health = Health::load()?;

    let health = if json {
        serde_json::to_string_pretty(&health).unwrap()
    } else {
        format!("{}", health)
    };

    println!("{}", health);

    Ok(())
}
