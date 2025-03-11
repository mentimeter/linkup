use std::{
    fmt::Write,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::{commands::local_dns, linkup_dir_path, linkup_file_path, local_config::LocalState};

use super::{get_running_pid, stop_pid_file, BackgroundService, Pid, PidError, Signal};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] PidError),
}

pub struct Dnsmasq {
    port: u16,
    config_file_path: PathBuf,
    log_file_path: PathBuf,
    pid_file_path: PathBuf,
}

impl Dnsmasq {
    pub fn new() -> Self {
        Self {
            port: 8053,
            config_file_path: linkup_file_path("dnsmasq-conf"),
            log_file_path: linkup_file_path("dnsmasq-log"),
            pid_file_path: linkup_file_path("dnsmasq-pid"),
        }
    }

    fn setup(&self, domains: &[String], linkup_session_name: &str) -> Result<(), Error> {
        let local_domains_template = domains.iter().fold(String::new(), |mut acc, d| {
            let _ = write!(
                acc,
                "address=/{0}.{1}/127.0.0.1\naddress=/{0}.{1}/::1\nlocal=/{0}.{1}/\n",
                linkup_session_name, d,
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

    pub fn stop(&self) {
        log::debug!("Stopping {}", Self::NAME);

        stop_pid_file(&self.pid_file_path, Signal::Term);
    }

    pub fn running_pid(&self) -> Option<Pid> {
        get_running_pid(&self.pid_file_path)
    }

    fn should_start(&self, domains: &[String]) -> Result<bool, Error> {
        let resolvers = local_dns::list_resolvers()?;

        Ok(domains.iter().any(|domain| resolvers.contains(domain)))
    }
}

impl BackgroundService<Error> for Dnsmasq {
    const NAME: &str = "Dnsmasq";

    async fn run_with_progress(
        &self,
        state: &mut LocalState,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        let domains = &state.domain_strings();

        match self.should_start(domains) {
            Ok(true) => (),
            Ok(false) => {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Skipped,
                    "Local DNS not installed",
                );

                return Ok(());
            }
            Err(err) => {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Skipped,
                    "Failed to read resolvers folder",
                );

                log::warn!("Failed to read resolvers folder: {}", err);

                return Ok(());
            }
        }

        self.notify_update(&status_sender, super::RunStatus::Starting);

        if self.running_pid().is_some() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Started,
                "Was already running",
            );

            return Ok(());
        }

        if let Err(e) = self.setup(domains, &state.linkup.session_name) {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to setup",
            );

            return Err(e);
        }

        if let Err(e) = self.start() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(e);
        }

        self.notify_update(&status_sender, super::RunStatus::Started);

        Ok(())
    }
}

pub fn is_installed() -> bool {
    let res = Command::new("which")
        .args(["dnsmasq"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .unwrap();

    res.success()
}
