use std::{
    collections::hash_map::DefaultHasher,
    fmt::Display,
    hash::{Hash, Hasher},
    sync,
};

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

pub trait BackgroundService {
    const ID: &str;
    const NAME: &str;

    async fn run_with_progress(
        &self,
        local_state: &mut State,
        status_sender: sync::mpsc::Sender<RunUpdate>,
    ) -> anyhow::Result<()>;

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

/// Returns an instance-scoped service ID. The resolution must mirror
/// `linkup_dir_path()` -- if a non-default directory is active, the ID
/// is hashed to prevent collisions between concurrent instances.
pub fn service_id(base_id: &str) -> String {
    if let Ok(home) = std::env::var("LINKUP_HOME") {
        return service_id_for_home(base_id, &home);
    }
    let current = crate::linkup_dir_path();
    let default = crate::default_linkup_dir_path();
    if current != default {
        return service_id_for_home(base_id, &current.to_string_lossy());
    }
    base_id.to_string()
}

/// Like `service_id`, but computes the scoped ID for a specific LINKUP_HOME path.
/// Used by instance management commands that need to stop services for other instances.
///
/// `DefaultHasher` is not stable across Rust versions, but this is fine:
/// service IDs only match running processes and are never persisted.
pub fn service_id_for_home(base_id: &str, linkup_home: &str) -> String {
    let mut hasher = DefaultHasher::new();
    linkup_home.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    format!("{}-{}", base_id, &hash[..8])
}

pub fn find_service_pid(service_id: &str) -> Option<Pid> {
    for (pid, process) in system().processes() {
        if process
            .environ()
            .iter()
            .any(|item| item.to_string_lossy() == format!("LINKUP_SERVICE_ID={service_id}"))
        {
            return Some(*pid);
        }
    }

    None
}

pub fn stop_service(service_id: &str) {
    if let Some(pid) = find_service_pid(service_id) {
        system()
            .process(pid)
            .map(|process| process.kill_with(Signal::Interrupt));
    }
}

pub fn system() -> System {
    System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_id_without_linkup_home() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();
        unsafe { std::env::remove_var("LINKUP_HOME") };

        let id = service_id("linkup-local-server");
        assert_eq!(id, "linkup-local-server");

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        }
    }

    #[test]
    fn test_service_id_with_linkup_home() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();
        unsafe { std::env::set_var("LINKUP_HOME", "/tmp/test-linkup-instance") };

        let id = service_id("linkup-local-server");
        assert!(id.starts_with("linkup-local-server-"));
        assert_ne!(id, "linkup-local-server");
        assert_eq!(id.len(), "linkup-local-server-".len() + 8);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }

    #[test]
    fn test_service_id_deterministic() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();
        unsafe { std::env::set_var("LINKUP_HOME", "/tmp/deterministic-test") };

        let id1 = service_id("test-service");
        let id2 = service_id("test-service");
        assert_eq!(id1, id2);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }

    #[test]
    fn test_service_id_consistent_with_service_id_for_home() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();
        let path = "/tmp/consistency-test";
        unsafe { std::env::set_var("LINKUP_HOME", path) };

        let via_env = service_id("linkup-local-server");
        let via_fn = service_id_for_home("linkup-local-server", path);
        assert_eq!(via_env, via_fn);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }

    #[test]
    fn test_service_id_different_homes_differ() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();

        unsafe { std::env::set_var("LINKUP_HOME", "/tmp/instance-a") };
        let id_a = service_id("linkup-local-server");

        unsafe { std::env::set_var("LINKUP_HOME", "/tmp/instance-b") };
        let id_b = service_id("linkup-local-server");

        assert_ne!(id_a, id_b);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }
}
