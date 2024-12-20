use std::{fmt::Display, sync};

mod caddy;
mod cloudflare_tunnel;
mod dnsmasq;
mod local_server;

pub use local_server::LocalServer;
pub use {caddy::is_installed as is_caddy_installed, caddy::Caddy};
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
