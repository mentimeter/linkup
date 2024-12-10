use std::{
    fs::{self, remove_file, File},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    sync::{Arc, Mutex},
    thread::sleep,
    time::Duration,
};

use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    proto::rr::RecordType,
    Resolver,
};
use regex::Regex;
use url::Url;

use crate::{linkup_file_path, local_config::LocalState, signal};

use super::{local_server::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum Error {
    #[error("Something went wrong...")] // TODO: Remove Default variant for specific ones
    Default,
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed while locking state file")]
    StateFileLock,
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
}

pub struct CloudflareTunnel {
    state: Arc<Mutex<LocalState>>,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl CloudflareTunnel {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            stdout_file_path: linkup_file_path("cloudflared-stdout"),
            stderr_file_path: linkup_file_path("cloudflared-stderr"),
            pidfile_path: linkup_file_path("cloudflared-pid"),
        }
    }

    pub fn url(&self) -> Url {
        let tunnel_url_re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")
            .expect("Failed to compile regex");

        let stderr_content = fs::read_to_string(&self.stderr_file_path)
            .map_err(|e| Error::from(e))
            .unwrap();

        match tunnel_url_re.find(&stderr_content) {
            Some(url_match) => {
                return Url::parse(url_match.as_str()).expect("Failed to parse tunnel URL");
            }
            None => panic!("failed to find tunnel url"),
        }
    }

    fn start(&self) -> Result<(), Error> {
        let _ = remove_file(&self.pidfile_path);

        let stdout_file = File::create(&self.stdout_file_path).map_err(|e| Error::from(e))?;
        let stderr_file = File::create(&self.stderr_file_path).map_err(|e| Error::from(e))?;

        let url = format!("http://localhost:{}", LINKUP_LOCAL_SERVER_PORT);

        process::Command::new("cloudflared")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .args(&[
                "tunnel",
                "--url",
                &url,
                "--pidfile",
                self.pidfile_path.to_str().unwrap(),
            ])
            .spawn()?;

        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);

        signal::stop_pid_file(&self.pidfile_path, signal::Signal::SIGINT)
            .map_err(|e| Error::from(e))?;

        Ok(())
    }

    fn dns_propagated(&self) -> bool {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(err) => {
                log::error!("Failed to aquire state lock: {}", err);

                return false;
            }
        };

        if let Some(tunnel) = &state.linkup.tunnel {
            log::debug!("Waiting for tunnel DNS to propagate at {}...", tunnel);

            let mut opts = ResolverOpts::default();
            opts.cache_size = 0; // Disable caching

            let resolver = Resolver::new(ResolverConfig::default(), opts).unwrap();

            let url = self.url();
            let domain = url.host_str().unwrap();

            let response = resolver.lookup(domain, RecordType::A);

            if let Ok(lookup) = response {
                let addresses = lookup.iter().collect::<Vec<_>>();

                if !addresses.is_empty() {
                    log::debug!("DNS has propogated for {}.", domain);
                    // thread::sleep(Duration::from_millis(1000));

                    return true;
                }
            }
        }

        return false;
    }

    fn update_state(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::StateFileLock)?;

        state.linkup.tunnel = Some(self.url());

        Ok(())
    }
}

impl BackgroundService<Error> for CloudflareTunnel {
    const NAME: &str = "Cloudflare Tunnel";

    fn run_with_progress(
        &self,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        self.notify_update(&status_sender, super::RunStatus::Starting);

        if let Err(_) = self.start() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(Error::Default);
        }

        // Pidfile existence check
        {
            let mut pid_file_ready_attempt = 0;
            let mut pid_file_exists = self.pidfile_path.exists();
            while !pid_file_exists && pid_file_ready_attempt < 10 {
                sleep(Duration::from_secs(1));
                pid_file_ready_attempt += 1;

                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Starting,
                    format!(
                        "Waiting for tunnel... retry #{}",
                        pid_file_ready_attempt + 1
                    ),
                );

                pid_file_exists = self.pidfile_path.exists();
            }

            if !pid_file_exists {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to start tunnel",
                );

                return Err(Error::Default);
            }
        }

        // DNS Propagation check
        {
            let mut dns_propagation_attempt = 0;
            let mut dns_propagated = self.dns_propagated();
            // TODO: Isn't 40 too much?
            while !dns_propagated && dns_propagation_attempt < 40 {
                sleep(Duration::from_secs(2));
                dns_propagation_attempt += 1;

                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Starting,
                    format!(
                        "Waiting for tunnel DNS to propagate... retry #{}",
                        dns_propagation_attempt + 1
                    ),
                );

                dns_propagated = self.dns_propagated();
            }

            if !dns_propagated {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to propagate tunnel DNS",
                );

                return Err(Error::Default);
            }
        }

        match self.update_state() {
            Ok(_) => {
                self.notify_update(&status_sender, super::RunStatus::Started);
            }
            Err(_) => {
                self.notify_update(&status_sender, super::RunStatus::Error);

                return Err(Error::Default);
            }
        }

        Ok(())
    }
}
