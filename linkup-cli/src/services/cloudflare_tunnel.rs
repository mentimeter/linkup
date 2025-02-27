use std::{
    env,
    fs::{self, File},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    time::Duration,
};

use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    proto::rr::RecordType,
    TokioAsyncResolver,
};
use log::{debug, error};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use url::Url;

use crate::{
    linkup_file_path, local_config::LocalState, signal, worker_client::WorkerClient,
    LINKUP_LOCALSERVER_PORT,
};

use super::BackgroundService;

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
    #[error("Failed to find tunnel URL")]
    UrlNotFound,
    #[error("Failed to find pidfile")]
    PidfileNotFound,
    #[error("Failed to verify that DNS got propagated")]
    DNSNotPropagated,
    #[error("Invalid session name: '{0}'")]
    InvalidSessionName(String),
}

pub struct CloudflareTunnel {
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl CloudflareTunnel {
    pub fn new() -> Self {
        Self {
            stdout_file_path: linkup_file_path("cloudflared-stdout"),
            stderr_file_path: linkup_file_path("cloudflared-stderr"),
            pidfile_path: linkup_file_path("cloudflared-pid"),
        }
    }

    async fn start(
        &self,
        worker_url: &Url,
        worker_token: &str,
        linkup_session_name: &str,
    ) -> Result<Url, Error> {
        let stdout_file = File::create(&self.stdout_file_path)?;
        let stderr_file = File::create(&self.stderr_file_path)?;

        log::info!(
            "Trying to acquire tunnel with name: {}",
            linkup_session_name
        );

        let worker_client = WorkerClient::new(worker_url, worker_token);
        let tunnel_data = worker_client
            .get_tunnel(linkup_session_name)
            .await
            .map_err(|e| Error::FailedToStart(e.to_string()))?;
        let tunnel_url = Url::parse(&tunnel_data.url).expect("tunnel_data url to be valid URL");

        save_tunnel_credentials(
            &tunnel_data.account_id,
            &tunnel_data.id,
            &tunnel_data.secret,
        )?;
        create_config_yml(&tunnel_data.id)?;

        log::debug!("Starting tunnel with name: {}", self.pidfile_path.display());
        log::debug!("Starting tunnel with name: {}", tunnel_data.name);

        process::Command::new("cloudflared")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .args([
                "tunnel",
                "--pidfile",
                self.pidfile_path
                    .to_str()
                    .expect("pidfile path to be valid UTF-8"),
                "run",
            ])
            .spawn()?;

        Ok(tunnel_url)
    }

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);

        signal::stop_pid_file(&self.pidfile_path, signal::Signal::SIGINT)?;

        Ok(())
    }

    pub fn running_pid(&self) -> Option<String> {
        signal::get_running_pid(&self.pidfile_path)
    }

    async fn dns_propagated(&self, tunnel_url: &Url) -> bool {
        let mut opts = ResolverOpts::default();
        opts.cache_size = 0; // Disable caching

        let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), opts);

        let domain = tunnel_url.host_str().unwrap();

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

        false
    }

    fn update_state(&self, tunnel_url: &Url, state: &mut LocalState) -> Result<(), Error> {
        debug!("Adding tunnel url {} to the state", tunnel_url.as_str());

        state.linkup.tunnel = Some(tunnel_url.clone());
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

            return Ok(());
        }

        if state.linkup.session_name.is_empty() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Empty session name",
            );

            return Err(Error::InvalidSessionName(state.linkup.session_name.clone()));
        }

        if self.running_pid().is_some() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Started,
                "Was already running",
            );

            return Ok(());
        }

        self.notify_update(&status_sender, super::RunStatus::Starting);

        let tunnel_url = self
            .start(
                &state.linkup.worker_url,
                &state.linkup.worker_token,
                &state.linkup.session_name,
            )
            .await;

        match tunnel_url {
            Ok(tunnel_url) => {
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

                        return Err(Error::PidfileNotFound);
                    }

                    self.notify_update(&status_sender, super::RunStatus::Starting);
                }

                // DNS Propagation check
                {
                    let mut dns_propagation_attempt = 0;
                    let mut dns_propagated = self.dns_propagated(&tunnel_url).await;

                    while !dns_propagated && dns_propagation_attempt <= 20 {
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

                        dns_propagated = self.dns_propagated(&tunnel_url).await;
                    }

                    if !dns_propagated {
                        self.notify_update_with_details(
                            &status_sender,
                            super::RunStatus::Error,
                            "Failed to propagate tunnel DNS",
                        );

                        return Err(Error::DNSNotPropagated);
                    }

                    self.notify_update(&status_sender, super::RunStatus::Starting);
                }

                match self.update_state(&tunnel_url, state) {
                    Ok(_) => {
                        self.notify_update(&status_sender, super::RunStatus::Started);
                    }
                    Err(e) => {
                        self.notify_update(&status_sender, super::RunStatus::Error);

                        return Err(e);
                    }
                }

                Ok(())
            }
            Err(e) => {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Error,
                    "Failed to start",
                );

                Err(e)
            }
        }
    }
}

pub fn is_installed() -> bool {
    let res = Command::new("which")
        .args(["cloudflared"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .unwrap();

    res.success()
}

fn save_tunnel_credentials(
    account_id: &str,
    tunnel_id: &str,
    tunnel_secret: &str,
) -> Result<(), Error> {
    let data = serde_json::json!({
        "AccountTag": account_id,
        "TunnelID": tunnel_id,
        "TunnelSecret": tunnel_secret,
    });

    let home_dir = env::var("HOME").expect("HOME environment variable to be present");
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let file_path = dir_path.join(format!("{}.json", tunnel_id));

    fs::write(&file_path, data.to_string())?;

    Ok(())
}

fn create_config_yml(tunnel_id: &str) -> Result<(), Error> {
    #[derive(Serialize, Deserialize)]
    struct Config {
        url: String,
        tunnel: String,
        #[serde(rename = "credentials-file")]
        credentials_file: String,
    }

    let home_dir = env::var("HOME").expect("HOME environment variable to be present");
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let credentials_file_path = dir_path.join(format!("{}.json", tunnel_id));
    let credentials_file_path_str = credentials_file_path.to_string_lossy().to_string();

    let config = Config {
        url: format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT),
        tunnel: tunnel_id.to_string(),
        credentials_file: credentials_file_path_str,
    };

    let serialized = serde_yaml::to_string(&config).expect("Failed to serialize config");

    fs::write(dir_path.join("config.yml"), serialized)?;

    Ok(())
}
