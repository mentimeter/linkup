use std::{
    collections::BTreeMap,
    env,
    io::{Write, stdout},
};

use anyhow::Result;
use clap::crate_version;
use colored::Colorize;
use linkup::Session;
use linkup_clients::LocalServerClient;
use serde::Serialize;

use crate::{
    services::{cloudflared, local_server},
    state::State,
};

use super::local_dns;

#[derive(clap::Args)]
pub struct Args {
    // Output status in JSON format
    #[arg(long)]
    pub json: bool,
}

pub async fn health(args: &Args) -> Result<()> {
    let health = Health::load().await?;

    let mut stdout = stdout();

    if args.json {
        stdout.write_all(serde_json::to_string_pretty(&health).unwrap().as_bytes())?;
    } else {
        health.write(&mut stdout)?;
    };

    Ok(())
}

#[derive(Serialize)]
struct Cli {
    version: String,
}

#[derive(Serialize)]
struct System {
    os: String,
    architecture: String,
    dns_resolvers: Vec<String>,
}

#[derive(Serialize)]
struct States {
    items: BTreeMap<String, State>,
}

#[derive(Serialize)]
enum LocalServer {
    Stopped,
    Running {
        pid: u32,
        healthy: bool,
        dns_records: Option<Vec<String>>,
        sessions: Option<BTreeMap<String, Session>>,
    },
}

#[derive(Serialize)]
enum Cloudflared {
    Stopped,
    Running { pid: u32 },
}

#[derive(Serialize)]
struct Health {
    cli: Cli,
    system: System,
    states: States,
    local_server: LocalServer,
    cloudflared: Cloudflared,
}

impl Cli {
    fn load() -> Result<Self> {
        let version = crate_version!().to_string();

        Ok(Self { version })
    }

    fn write(&self, writer: &mut impl Write, offset: usize) -> Result<()> {
        writeln!(writer, "{:>offset$}Version: {}", "", self.version)?;
        Ok(())
    }
}

impl System {
    fn load() -> Result<Self> {
        Ok(Self {
            os: sysinfo::System::name().unwrap(),
            architecture: env::consts::ARCH.to_string(),
            dns_resolvers: local_dns::list_resolvers()?,
        })
    }

    fn write(&self, writer: &mut impl Write, offset: usize) -> Result<()> {
        writeln!(writer, "{:>offset$}OS: {}", "", self.os)?;
        writeln!(writer, "{:>offset$}Architecture: {}", "", self.architecture)?;

        write!(writer, "{:>offset$}DNS Resolvers:", "")?;
        if self.dns_resolvers.is_empty() {
            writeln!(writer, " {}", "NONE".yellow())?;
        } else {
            writeln!(writer)?;
            for resolver in &self.dns_resolvers {
                writeln!(writer, "{:>offset$}- {}", "", resolver, offset = offset + 2)?;
            }
        }

        Ok(())
    }
}

impl States {
    fn load() -> Result<Self> {
        let mut items = BTreeMap::new();

        if let Ok(state) = State::load() {
            items.insert("state".to_string(), state);
        }

        Ok(Self { items })
    }

    fn write(&self, writer: &mut impl Write, offset: usize) -> Result<()> {
        if self.items.is_empty() {
            writeln!(writer, " {}", "NONE".yellow())?;
            return Ok(());
        }

        writeln!(writer)?;
        for (state_file_name, state) in &self.items {
            writeln!(
                writer,
                "{:>offset$}- [{}] {} ({})",
                "", state_file_name, state.linkup.session_name, state.linkup.kind,
            )?;
        }

        Ok(())
    }
}

impl LocalServer {
    async fn load() -> Result<Self> {
        let local_server_client = LocalServerClient::new(&local_server::url());

        match local_server::find_pid() {
            Some(pid) => {
                let is_healthy = local_server_client.health_check().await.unwrap_or_default();
                let dns_records = local_server_client
                    .list_dns_domains()
                    .await
                    .map(|res| res.domains)
                    .ok();
                let sessions = local_server_client
                    .list_sessions()
                    .await
                    .map(|res| res.sessions.into_iter().collect::<BTreeMap<_, _>>())
                    .ok();

                Ok(Self::Running {
                    pid: pid.as_u32(),
                    healthy: is_healthy,
                    dns_records,
                    sessions,
                })
            }
            None => Ok(Self::Stopped),
        }
    }

    fn write(&self, writer: &mut impl Write, offset: usize) -> Result<()> {
        match &self {
            LocalServer::Stopped => {
                write!(writer, " {}", "NOT RUNNING".yellow())?;
                writeln!(writer)?;
            }
            LocalServer::Running {
                pid,
                healthy,
                dns_records,
                sessions,
            } => {
                writeln!(writer)?;

                write!(writer, "{:>offset$}Health:", "")?;
                if *healthy {
                    writeln!(writer, " {}", "OK".blue())?;
                } else {
                    writeln!(writer, " {}", "ERR".yellow())?;
                }

                writeln!(writer, "{:>offset$}PID: {}", "", pid)?;

                write!(writer, "{:>offset$}Sessions:", "")?;
                match sessions {
                    Some(sessions) => {
                        writeln!(writer)?;
                        for (session_name, session) in sessions {
                            writeln!(
                                writer,
                                "{:>offset$}- {} ({})",
                                "",
                                session_name,
                                session.kind,
                                offset = offset + 2
                            )?;
                        }
                    }
                    None => writeln!(writer, " {}", "FAILED TO FETCH".yellow())?,
                }

                write!(writer, "{:>offset$}DNS Records:", "")?;
                match dns_records {
                    Some(dns_records) => {
                        writeln!(writer)?;
                        for dns_record in dns_records {
                            writeln!(
                                writer,
                                "{:>offset$}- {}",
                                "",
                                dns_record,
                                offset = offset + 2
                            )?;
                        }
                    }
                    None => writeln!(writer, " {}", "FAILED TO FETCH".yellow())?,
                }
            }
        }

        Ok(())
    }
}

impl Cloudflared {
    fn load() -> Result<Self> {
        match cloudflared::find_pid() {
            Some(pid) => Ok(Self::Running { pid: pid.as_u32() }),
            None => Ok(Self::Stopped),
        }
    }

    fn write(&self, writer: &mut impl Write, offset: usize) -> Result<()> {
        match &self {
            Self::Stopped => {
                write!(writer, " {}", "NOT RUNNING".yellow())?;
                writeln!(writer)?;
            }
            Self::Running { pid } => {
                writeln!(writer)?;
                writeln!(writer, "{:>offset$}PID: {}", "", pid)?;
            }
        }

        Ok(())
    }
}

impl Health {
    async fn load() -> Result<Self> {
        Ok(Self {
            cli: Cli::load()?,
            system: System::load()?,
            states: States::load()?,
            local_server: LocalServer::load().await?,
            cloudflared: Cloudflared::load()?,
        })
    }

    fn write(&self, writer: &mut impl Write) -> Result<()> {
        writeln!(writer, "{}", "System:".bold())?;
        self.system.write(writer, 2)?;

        writeln!(writer, "{}", "CLI:".bold())?;
        self.cli.write(writer, 2)?;

        write!(writer, "{}", "States:".bold())?;
        self.states.write(writer, 2)?;

        write!(writer, "{}", "Local Server:".bold())?;
        self.local_server.write(writer, 2)?;

        write!(writer, "{}", "Cloudflared:".bold())?;
        self.cloudflared.write(writer, 2)?;

        Ok(())
    }
}
