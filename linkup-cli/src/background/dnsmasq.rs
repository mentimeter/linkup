use std::{
    fmt::Write,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use nix::sys::signal::Signal;

use crate::{
    linkup_dir_path, linkup_file_path,
    local_config::LocalState,
    signal::{self, get_running_pid},
};

use super::{stop_pid_file, BackgroundService};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed while locking state file")]
    StateFileLock,
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
}

pub struct Dnsmasq {
    state: Arc<Mutex<LocalState>>,
    port: u16,
    config_file_path: PathBuf,
    log_file_path: PathBuf,
    pid_file_path: PathBuf,
}

impl Dnsmasq {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            port: 8053,
            config_file_path: linkup_file_path("dnsmasq-conf"),
            log_file_path: linkup_file_path("dnsmasq-log"),
            pid_file_path: linkup_file_path("dnsmasq-pid"),
        }
    }
}

impl BackgroundService for Dnsmasq {
    fn name(&self) -> String {
        String::from("Dnsmasq")
    }

    fn setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.state.lock().map_err(|_| Error::StateFileLock)?;
        let session_name = state.linkup.session_name.clone();

        let local_domains_template =
            state
                .domain_strings()
                .iter()
                .fold(String::new(), |mut acc, d| {
                    let _ = write!(
                        acc,
                        "address=/{}.{}/127.0.0.1\naddress=/{}.{}/::1\nlocal=/{}.{}/\n",
                        session_name, d, session_name, d, session_name, d
                    );
                    acc
                });

        let dnsmasq_template = format!(
            "{}

port={}
log-facility={}
pid-file={}\n",
            local_domains_template,
            self.port,
            self.log_file_path.display(),
            self.pid_file_path.display(),
        );

        fs::write(&self.config_file_path, dnsmasq_template)?;

        Ok(())
    }

    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("Starting {}", self.name());

        Command::new("dnsmasq")
            .current_dir(linkup_dir_path())
            .arg("--log-queries")
            .arg("-C")
            .arg(&self.config_file_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;

        Ok(())
    }

    fn ready(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    fn update_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("Stopping {}", self.name());

        stop_pid_file(&self.pid_file_path, Signal::SIGTERM)?;

        Ok(())
    }

    fn pid(&self) -> Option<String> {
        get_running_pid(&self.pid_file_path)
    }
}
