use std::{fmt::Display, path::Path};

use nix::sys::signal::Signal;

use crate::{
    signal::{get_pid, send_signal, PidError},
    CliError,
};

mod caddy;
mod cloudflare_tunnel;
mod dnsmasq;
mod localserver;

pub use {
    caddy::Caddy, cloudflare_tunnel::CloudflareTunnel, dnsmasq::Dnsmasq, localserver::LocalServer,
};

pub trait BackgroundService {
    fn name(&self) -> String;
    fn setup(&self);
    fn start(&self);
    fn ready(&self) -> bool;
    fn update_state(&self);
    fn stop(&self);
    fn pid(&self) -> Option<String>;
}

#[derive(Clone, PartialEq, Eq)]
pub enum BackgroudServiceStatus {
    Pending,
    Starting,
    Started,
    Timeout,
}

impl Display for BackgroudServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackgroudServiceStatus::Pending => write!(f, "{}", "pending"),
            BackgroudServiceStatus::Starting => write!(f, "{}", "starting"),
            BackgroudServiceStatus::Started => write!(f, "{}", "started"),
            BackgroudServiceStatus::Timeout => write!(f, "{}", "timeout"),
        }
    }
}

pub fn stop_pid_file(pid_file: &Path, signal: Signal) -> Result<(), CliError> {
    let stopped = match get_pid(pid_file) {
        Ok(pid) => match send_signal(&pid, signal) {
            Ok(_) => Ok(()),
            Err(PidError::NoSuchProcess(_)) => Ok(()),
            Err(e) => Err(CliError::StopErr(format!(
                "Could not send {} to {} pid {}: {}",
                signal,
                pid_file.display(),
                pid,
                e
            ))),
        },
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get {} pid: {}",
            pid_file.display(),
            e
        ))),
    };

    if stopped.is_ok() {
        let _ = std::fs::remove_file(pid_file);
    }

    stopped
}
