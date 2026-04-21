use std::{fmt::Display, ops::Deref};

use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use thiserror::Error;

pub mod cloudflared;
pub mod local_server;

pub use sysinfo::{Pid, Signal};

pub struct ServiceId(&'static str);

#[derive(Error, Debug)]
pub enum PidError {
    #[error("no pid file: {0}")]
    NoPidFile(String),
    #[error("bad pid file: {0}")]
    BadPidFile(String),
}

impl Deref for ServiceId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl Display for ServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn system() -> System {
    System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    )
}

fn find_pid(service_id: ServiceId) -> Option<Pid> {
    for (pid, process) in system().processes() {
        if process
            .environ()
            .iter()
            .any(|item| item.to_string_lossy() == format!("LINKUP_SERVICE_ID={}", service_id))
        {
            return Some(*pid);
        }
    }

    None
}

fn stop(service_id: ServiceId) {
    if let Some(pid) = find_pid(service_id) {
        system()
            .process(pid)
            .map(|process| process.kill_with(Signal::Interrupt));
    }
}
