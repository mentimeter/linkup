use std::{
    env,
    fs::{self, File},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    time::Duration,
};

use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    proto::rr::RecordType,
    TokioAsyncResolver,
};
use log::{debug, error};
use regex::Regex;
use tokio::time::sleep;
use url::Url;

use crate::{linkup_file_path, local_config::LocalState, paid_tunnel, signal};

use super::{local_server::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum Error {
    #[error("Something went wrong...")] // TODO: Remove Default variant for specific ones
    Default,
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
    #[error("Failed to find tunnel URL")]
    UrlNotFound,
}

pub struct CloudflareTunnel {
    linkup_session_name: String,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl CloudflareTunnel {
    pub fn new(linkup_session_name: impl Into<String>) -> Self {
        Self {
            linkup_session_name: linkup_session_name.into(),
            stdout_file_path: linkup_file_path("cloudflared-stdout"),
            stderr_file_path: linkup_file_path("cloudflared-stderr"),
            pidfile_path: linkup_file_path("cloudflared-pid"),
        }
    }

    fn use_paid_tunnels(&self) -> bool {
        env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").is_ok()
            && env::var("LINKUP_CLOUDFLARE_ZONE_ID").is_ok()
            && env::var("LINKUP_CF_API_TOKEN").is_ok()
    }

    fn start_free(&self) -> Result<(), Error> {
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

    async fn start_paid(&self) -> Result<(), Error> {
        let stdout_file = File::create(&self.stdout_file_path).map_err(|e| Error::from(e))?;
        let stderr_file = File::create(&self.stderr_file_path).map_err(|e| Error::from(e))?;

        log::info!(
            "Starting paid tunnel with session name: {}",
            self.linkup_session_name
        );

        let tunnel_name = format!("tunnel-{}", self.linkup_session_name);
        let mut tunnel_id = match paid_tunnel::get_tunnel_id(&tunnel_name).await {
            Ok(Some(id)) => id,
            Ok(None) => "".to_string(),
            // Err(e) => return Err(e),
            Err(e) => panic!("{}", e),
        };

        let mut create_tunnel = false;

        if tunnel_id.is_empty() {
            log::info!("Tunnel ID is empty");

            create_tunnel = true;
        } else {
            log::info!("Tunnel ID: {}", tunnel_id);

            let file_path = format!(
                "{}/.cloudflared/{}.json",
                std::env::var("HOME").unwrap(),
                tunnel_id
            );

            if fs::exists(&file_path).unwrap_or(false) {
                log::info!("Tunnel config file for {}: {}", tunnel_id, file_path);
            } else {
                log::info!("Tunnel config file for {} does not exist", tunnel_id);

                create_tunnel = true;
            }
        }

        if create_tunnel {
            println!("Creating tunnel...");

            tunnel_id = paid_tunnel::create_tunnel(&tunnel_name).await.unwrap();
            paid_tunnel::create_dns_record(&tunnel_id, &tunnel_name)
                .await
                .unwrap();
        }

        process::Command::new("cloudflared")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .args(&[
                "tunnel",
                "--pidfile",
                self.pidfile_path.to_str().unwrap(),
                "run",
                &tunnel_name,
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

    fn url(&self) -> Result<Url, Error> {
        if self.use_paid_tunnels() {
            return Ok(Url::parse(
                format!(
                    "https://tunnel-{}.mentimeter.dev",
                    &self.linkup_session_name
                )
                .as_str(),
            )
            .expect("Failed to parse tunnel URL"));
        } else {
            let tunnel_url_re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")
                .expect("Failed to compile regex");

            let stderr_content = fs::read_to_string(&self.stderr_file_path)
                .map_err(|e| Error::from(e))
                .unwrap();

            match tunnel_url_re.find(&stderr_content) {
                Some(url_match) => {
                    return Ok(Url::parse(url_match.as_str()).expect("Failed to parse tunnel URL"));
                }
                None => return Err(Error::UrlNotFound),
            }
        }
    }

    async fn dns_propagated(&self) -> bool {
        let mut opts = ResolverOpts::default();
        opts.cache_size = 0; // Disable caching

        let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), opts);

        let url = match self.url() {
            Ok(url) => url,
            Err(_) => return false,
        };

        // let url = self.url().unwrap_or_else(|_| return false);
        let domain = url.host_str().unwrap();

        let response = resolver.lookup(domain, RecordType::A).await;

        if let Ok(lookup) = response {
            let addresses = lookup.iter().collect::<Vec<_>>();

            if !addresses.is_empty() {
                log::debug!("DNS has propogated for {}.", domain);

                return true;
            }
        } else {
            log::debug!("DNS {} not propagated yet.", domain);
        }

        return false;
    }

    fn update_state(&self, state: &mut LocalState) -> Result<(), Error> {
        let url = self.url()?;

        debug!("Adding tunne url {} to the state", url.as_str());

        state.linkup.tunnel = Some(url);
        state
            .save()
            .expect("failed to update local state file with tunnel url");

        Ok(())
    }
}

impl BackgroundService<Error> for CloudflareTunnel {
    const NAME: &str = "Cloudflare Tunnel";

    async fn run_with_progress(
        &self,
        state: &mut LocalState,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        if !state.should_use_tunnel() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Skipped,
                "Requested no tunnel",
            );
        }

        if self.use_paid_tunnels() {
            self.notify_update_with_details(&status_sender, super::RunStatus::Starting, "Paid");

            if let Err(_) = self.start_paid().await {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to start",
                );

                return Err(Error::Default);
            }
        } else {
            self.notify_update_with_details(&status_sender, super::RunStatus::Starting, "Free");

            if let Err(_) = self.start_free() {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to start",
                );

                return Err(Error::Default);
            }
        }

        // Pidfile existence check
        {
            let mut pid_file_ready_attempt = 0;
            let mut pid_file_exists = signal::get_running_pid(&self.pidfile_path).is_some();
            while !pid_file_exists && pid_file_ready_attempt <= 10 {
                sleep(Duration::from_secs(1)).await;
                pid_file_ready_attempt += 1;

                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Starting,
                    format!("Waiting for tunnel... retry #{}", pid_file_ready_attempt),
                );

                pid_file_exists = signal::get_running_pid(&self.pidfile_path).is_some();
            }

            if !pid_file_exists {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to start tunnel",
                );

                return Err(Error::Default);
            }

            self.notify_update(&status_sender, super::RunStatus::Starting);
        }

        // DNS Propagation check
        {
            let mut dns_propagation_attempt = 0;
            let mut dns_propagated = self.dns_propagated().await;
            // TODO: Isn't 40 too much?
            while !dns_propagated && dns_propagation_attempt <= 40 {
                sleep(Duration::from_secs(2)).await;
                dns_propagation_attempt += 1;

                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Starting,
                    format!(
                        "Waiting for tunnel DNS to propagate... retry #{}",
                        dns_propagation_attempt
                    ),
                );

                dns_propagated = self.dns_propagated().await;
            }

            if !dns_propagated {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to propagate tunnel DNS",
                );

                return Err(Error::Default);
            }

            self.notify_update(&status_sender, super::RunStatus::Starting);
        }

        match self.update_state(state) {
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
