use std::{fmt::Display, path::Path};

use nix::sys::signal::Signal;

use crate::signal::{self, get_pid, send_signal, PidError};

mod caddy;
mod cloudflare_tunnel;
mod dnsmasq;
mod localserver;

pub use {
    caddy::Caddy, cloudflare_tunnel::CloudflareTunnel, dnsmasq::Dnsmasq, localserver::LocalServer,
};

pub trait BackgroundService {
    fn name(&self) -> String;
    fn setup(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn start(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn ready(&self) -> Result<bool, Box<dyn std::error::Error>>;
    fn update_state(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn stop(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn pid(&self) -> Option<String>;
}

#[derive(Clone, PartialEq, Eq)]
pub enum BackgroudServiceStatus {
    Pending,
    Starting,
    Started,
    Timeout,
    Error,
}

impl Display for BackgroudServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackgroudServiceStatus::Pending => write!(f, "{}", "pending"),
            BackgroudServiceStatus::Starting => write!(f, "{}", "starting"),
            BackgroudServiceStatus::Started => write!(f, "{}", "started"),
            BackgroudServiceStatus::Timeout => write!(f, "{}", "timeout"),
            BackgroudServiceStatus::Error => write!(f, "{}", "error"),
        }
    }
}

pub fn stop_pid_file(pid_file: &Path, signal: Signal) -> Result<(), signal::PidError> {
    let stopped = match get_pid(pid_file) {
        Ok(pid) => match send_signal(&pid, signal) {
            Ok(_) => Ok(()),
            Err(PidError::NoSuchProcess(_)) => Ok(()),
            Err(e) => Err(e),
        },
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(e),
    };

    if stopped.is_ok() {
        let _ = std::fs::remove_file(pid_file);
    }

    stopped
}
