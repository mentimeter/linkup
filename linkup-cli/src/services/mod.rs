use std::{fmt::Display, sync::LazyLock, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use thiserror::Error;

mod cloudflare_tunnel;
mod local_server;

pub use local_server::LocalServer;
pub use sysinfo::{Pid, Signal};
pub use {
    cloudflare_tunnel::CloudflareTunnel,
    cloudflare_tunnel::is_installed as is_cloudflared_installed,
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

pub trait BackgroundService {
    const ID: &str;
    const NAME: &str;

    fn prepare_progress_bar(&self, progress_bar: &ProgressBar) {
        progress_bar.set_prefix(Self::NAME);
        progress_bar.set_style(STATIC_STYLE.clone());
        progress_bar.set_message(RunStatus::Pending.to_string());
    }

    async fn run_with_progress(
        &self,
        local_state: &mut State,
        progress_bar: &ProgressBar,
    ) -> anyhow::Result<()>;

    fn stop() {
        if let Some(pid) = Self::find_pid() {
            system()
                .process(pid)
                .map(|process| process.kill_with(Signal::Interrupt));
        }
    }

    fn notify_update(&self, progress_bar: &ProgressBar, status: RunStatus) {
        match status {
            RunStatus::Starting => {
                progress_bar.set_style(IN_PROGRESS_STYLE.clone());
                progress_bar.enable_steady_tick(Duration::from_millis(50));
            }
            _ => progress_bar.set_style(STATIC_STYLE.clone()),
        }

        progress_bar.set_message(status.to_string());
    }

    fn notify_update_with_details(
        &self,
        progress_bar: &ProgressBar,
        status: RunStatus,
        details: impl Display,
    ) {
        progress_bar.set_message(format!("{status} ({details})"));

        match status {
            RunStatus::Starting => {
                progress_bar.enable_steady_tick(Duration::from_millis(50));
                progress_bar.set_style(IN_PROGRESS_STYLE.clone())
            }
            _ => progress_bar.set_style(STATIC_STYLE.clone()),
        }
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

static STATIC_STYLE: LazyLock<ProgressStyle> =
    LazyLock::new(|| ProgressStyle::with_template("{prefix:<20} {msg}").unwrap());

static IN_PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{prefix:<20} {spinner:.blue}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
});
