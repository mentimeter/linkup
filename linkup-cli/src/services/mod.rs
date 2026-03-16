use std::{fmt::Display, sync};

use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use thiserror::Error;

mod cloudflare_tunnel;
mod local_dns_server;
mod local_server;

pub use local_dns_server::LocalDnsServer;
pub use local_server::LocalServer;
pub use sysinfo::{Pid, Signal};
pub use {
    cloudflare_tunnel::is_installed as is_cloudflared_installed,
    cloudflare_tunnel::CloudflareTunnel,
};

use crate::state::State;

#[derive(Clone)]
pub enum RunStatus {
    Pending,
    Starting,
    Started,
    Skipped,
    Error,
}

#[derive(Error, Debug)]
pub enum PidError {
    #[error("no pid file: {0}")]
    NoPidFile(String),
    #[error("bad pid file: {0}")]
    BadPidFile(String),
}

#[derive(Clone)]
pub struct RunUpdate {
    pub id: String,
    pub status: RunStatus,
    pub details: Option<String>,
}

pub trait BackgroundService {
    const ID: &str;
    const NAME: &str;

    async fn run_with_progress(
        &self,
        local_state: &mut State,
        status_sender: sync::mpsc::Sender<RunUpdate>,
    ) -> anyhow::Result<()>;

    fn stop() {
        if let Some(pid) = Self::find_pid() {
            system()
                .process(pid)
                .map(|process| process.kill_with(Signal::Interrupt));
        }
    }

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

    fn find_pid() -> Option<Pid> {
        for (pid, process) in system().processes() {
            if process
                .environ()
                .iter()
                .any(|item| item.to_string_lossy() == format!("LINKUP_SERVICE_ID={}", Self::ID))
            {
                return Some(*pid);
            }
        }

        None
    }
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

pub fn system() -> System {
    System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    )
}
