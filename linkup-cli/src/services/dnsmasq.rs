use std::{
    fmt::Write,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use crate::{linkup_dir_path, linkup_file_path, local_config::LocalState, signal};

use super::BackgroundService;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Something went wrong...")] // TODO: Remove Default variant for specific ones
    Default,
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

    fn setup(&self) -> Result<(), Error> {
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

    fn start(&self) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

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

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);

        signal::stop_pid_file(&self.pid_file_path, signal::Signal::SIGTERM)?;

        Ok(())
    }
}

impl BackgroundService<Error> for Dnsmasq {
    const NAME: &str = "Dnsmasq";

    async fn run_with_progress(
        &self,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        self.notify_update(&status_sender, super::RunStatus::Starting);

        if let Err(_) = self.setup() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to setup",
            );

            return Err(Error::Default);
        }

        if let Err(_) = self.start() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(Error::Default);
        }

        self.notify_update(&status_sender, super::RunStatus::Started);

        Ok(())
    }
}
