use std::{
    env,
    fmt::Display,
    fs::{self},
};

use colored::Colorize;

use crate::{linkup_dir_path, local_config::LocalState, CliError};

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
struct LinkupDir {
    location: String,
    content: Vec<String>,
}

impl LinkupDir {
    fn load() -> Result<Self, CliError> {
        let dir_path = linkup_dir_path();
        let files: Vec<String> = fs::read_dir(&dir_path)?
            .map(|f| f.unwrap().file_name().to_str().unwrap().to_string())
            .collect();

        Ok(Self {
            location: dir_path.to_str().unwrap_or_default().to_string(),
            content: files,
        })
    }
}

#[derive(Debug)]
struct Health {
    system: System,
    session: Session,
    environment_variables: EnvironmentVariables,
    background_services: BackgroudServices,
    linkup_dir: LinkupDir,
}

impl Health {
    pub fn load() -> Result<Self, CliError> {
        Ok(Self {
            system: System::load(),
            session: Session::load()?,
            environment_variables: EnvironmentVariables::load(),
            background_services: BackgroudServices::load(),
            linkup_dir: LinkupDir::load()?,
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

        writeln!(f, "{}", "Linkup dir:".bold().italic())?;
        writeln!(f, "  Location: {}", self.linkup_dir.location)?;
        writeln!(f, "  Content:")?;
        for file in &self.linkup_dir.content {
            writeln!(f, "    - {}", file)?;
        }

        Ok(())
    }
}

pub fn health() -> Result<(), CliError> {
    let health = Health::load()?;
    println!("{}", health);

    Ok(())
}
