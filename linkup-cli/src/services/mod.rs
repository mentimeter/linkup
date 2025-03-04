use std::fs::{self, File};
use std::path::Path;
use std::{fmt::Display, sync};

use sysinfo::{get_current_pid, ProcessRefreshKind, RefreshKind, System};
use thiserror::Error;

mod caddy;
mod cloudflare_tunnel;
mod dnsmasq;
mod local_server;

pub use local_server::LocalServer;
pub use sysinfo::{Pid, Signal};
pub use {caddy::get_path as caddy_path, caddy::is_installed as is_caddy_installed, caddy::Caddy};
pub use {
    cloudflare_tunnel::is_installed as is_cloudflared_installed,
    cloudflare_tunnel::CloudflareTunnel,
};
pub use {dnsmasq::is_installed as is_dnsmasq_installed, dnsmasq::Dnsmasq};

use crate::local_config::LocalState;

#[derive(Clone)]
pub enum RunStatus {
    Pending,
    Starting,
    Started,
    Skipped,
    Error,
}

impl Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Starting => write!(f, "starting"),
            Self::Started => write!(f, "started"),
            Self::Skipped => write!(f, "skipped"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Clone)]
pub struct RunUpdate {
    pub id: String,
    pub status: RunStatus,
    pub details: Option<String>,
}

pub trait BackgroundService<E: std::error::Error> {
    const NAME: &str;

    async fn run_with_progress(
        &self,
        local_state: &mut LocalState,
        status_sender: sync::mpsc::Sender<RunUpdate>,
    ) -> Result<(), E>;

    fn notify_update(&self, status_sender: &sync::mpsc::Sender<RunUpdate>, status: RunStatus) {
        status_sender
            .send(RunUpdate {
                id: String::from(Self::NAME),
                status,
                details: None,
            })
            .unwrap();
    }

    fn notify_update_with_details(
        &self,
        status_sender: &sync::mpsc::Sender<RunUpdate>,
        status: RunStatus,
        details: impl Into<String>,
    ) {
        status_sender
            .send(RunUpdate {
                id: String::from(Self::NAME),
                status,
                details: Some(details.into()),
            })
            .unwrap();
    }
}

#[derive(Error, Debug)]
pub enum PidError {
    #[error("no pid file: {0}")]
    NoPidFile(String),
    #[error("bad pid file: {0}")]
    BadPidFile(String),
}

fn get_pid(file_path: &Path) -> Result<Pid, PidError> {
    if let Err(e) = File::open(file_path) {
        return Err(PidError::NoPidFile(e.to_string()));
    }

    match fs::read_to_string(file_path) {
        Ok(content) => {
            let pid_u32 = content
                .trim()
                .parse::<u32>()
                .map_err(|e| PidError::BadPidFile(e.to_string()))?;

            Ok(Pid::from_u32(pid_u32))
        }
        Err(e) => Err(PidError::BadPidFile(e.to_string())),
    }
}

// Get the pid from a pidfile, but only return Some in case the pidfile is valid and the written pid on the file
// is running.
pub fn get_running_pid(file_path: &Path) -> Option<Pid> {
    let pid = match get_pid(file_path) {
        Ok(pid) => pid,
        Err(_) => return None,
    };

    system().process(pid).map(|_| pid)
}

pub fn stop_pid_file(pid_file: &Path, signal: Signal) {
    if let Some(pid) = get_running_pid(pid_file) {
        system()
            .process(pid)
            .map(|process| process.kill_with(signal));
    }
}

pub fn system() -> System {
    System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    )
}

pub fn get_current_process_pid() -> Pid {
    get_current_pid().unwrap()
}
