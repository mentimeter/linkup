use std::{fmt::Display, sync};

pub mod tunnel;

mod caddy;
mod cloudflare_tunnel;
mod dnsmasq;
mod local_server;

pub use caddy::Caddy;
pub use cloudflare_tunnel::CloudflareTunnel;
pub use dnsmasq::Dnsmasq;
pub use local_server::LocalServer;

use crate::local_config::LocalState;

#[derive(Clone)]
pub enum RunStatus {
    Pending,
    Starting,
    Started,
    Error,
}

impl Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "{}", "pending"),
            Self::Starting => write!(f, "{}", "starting"),
            Self::Started => write!(f, "{}", "started"),
            Self::Error => write!(f, "{}", "error"),
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
