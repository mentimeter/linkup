use std::{
    fs::{self, File},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    time::Duration,
};

use hickory_resolver::{config::ResolverOpts, proto::rr::RecordType, TokioResolver};
use log::debug;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use url::Url;

use crate::{linkup_dir_path, linkup_file_path, state::State, worker_client::WorkerClient, Result};

use super::{find_service_pid, BackgroundService, PidError};

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] PidError),
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
        local_server_port: u16,
    ) -> Result<Url> {
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
        create_config_yml(&tunnel_data.id, local_server_port)?;

        log::debug!("Starting tunnel with name: {}", self.pidfile_path.display());
        log::debug!("Starting tunnel with name: {}", tunnel_data.name);

        let config_path = linkup_dir_path().join("cloudflared-config.yml");

        process::Command::new("cloudflared")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .env("LINKUP_SERVICE_ID", super::service_id(Self::ID))
            .args([
                "tunnel",
                "--config",
                config_path.to_str().expect("config path to be valid UTF-8"),
                "--pidfile",
                self.pidfile_path
                    .to_str()
                    .expect("pidfile path to be valid UTF-8"),
                "run",
            ])
            .spawn()?;

        Ok(tunnel_url)
    }

    async fn dns_propagated(&self, tunnel_url: &Url) -> bool {
        let mut opts = ResolverOpts::default();
        opts.cache_size = 0; // Disable caching

        let resolver = TokioResolver::builder_tokio()
            .expect("TokioResolver to be buildable from resolver config")
            .with_options(opts)
            .build();

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

    fn update_state(&self, tunnel_url: &Url, state: &mut State) -> Result<()> {
        debug!("Adding tunnel url {} to the state", tunnel_url.as_str());

        state.linkup.tunnel = Some(tunnel_url.clone());
        state
            .save()
            .expect("failed to update local state file with tunnel url");

        Ok(())
    }
}

impl BackgroundService for CloudflareTunnel {
    const ID: &str = "cloudflare-tunnel";
    const NAME: &str = "Cloudflare Tunnel";

    async fn run_with_progress(
        &self,
        state: &mut State,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<()> {
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

            return Err(Error::InvalidSessionName(state.linkup.session_name.clone()).into());
        }

        if find_service_pid(&super::service_id(Self::ID)).is_some() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Started,
                "Was already running",
            );

            return Ok(());
        }

        self.notify_update(&status_sender, super::RunStatus::Starting);

        let local_server_port = state.linkup.local_server_port.unwrap_or(80);
        let tunnel_url = self
            .start(
                &state.linkup.worker_url,
                &state.linkup.worker_token,
                &state.linkup.session_name,
                local_server_port,
            )
            .await;

        match tunnel_url {
            Ok(tunnel_url) => {
                // Pidfile existence check
                {
                    let mut pid_file_ready_attempt = 0;
                    let mut pid_file_exists = get_running_pid(&self.pidfile_path).is_some();
                    while !pid_file_exists && pid_file_ready_attempt <= 10 {
                        sleep(Duration::from_secs(1)).await;
                        pid_file_ready_attempt += 1;

                        self.notify_update_with_details(
                            &status_sender,
                            super::RunStatus::Starting,
                            format!("Waiting for tunnel... retry #{}", pid_file_ready_attempt),
                        );

                        pid_file_exists = get_running_pid(&self.pidfile_path).is_some();
                    }

                    if !pid_file_exists {
                        self.notify_update_with_details(
                            &status_sender,
                            super::RunStatus::Error,
                            "Failed to start tunnel",
                        );

                        return Err(Error::PidfileNotFound.into());
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

                        return Err(Error::DNSNotPropagated.into());
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

/// Credentials are stored in LINKUP_HOME (not ~/.cloudflared/) so each
/// instance gets its own tunnel identity.
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

    let dir_path = linkup_dir_path();

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let file_path = dir_path.join("cloudflared-creds.json");

    fs::write(&file_path, data.to_string())?;

    Ok(())
}

fn create_config_yml(tunnel_id: &str, local_server_port: u16) -> Result<(), Error> {
    #[derive(Serialize, Deserialize)]
    struct Config {
        url: String,
        tunnel: String,
        #[serde(rename = "credentials-file")]
        credentials_file: String,
    }

    let dir_path = linkup_dir_path();

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let credentials_file_path = dir_path.join("cloudflared-creds.json");
    let credentials_file_path_str = credentials_file_path.to_string_lossy().to_string();

    let config = Config {
        url: format!("http://localhost:{}", local_server_port),
        tunnel: tunnel_id.to_string(),
        credentials_file: credentials_file_path_str,
    };

    let serialized = serde_yaml::to_string(&config).expect("Failed to serialize config");

    fs::write(dir_path.join("cloudflared-config.yml"), serialized)?;

    Ok(())
}

// Get the pid from a pidfile, but only return Some in case the pidfile is valid and the written pid on the file
// is running.
fn get_running_pid(file_path: &Path) -> Option<super::Pid> {
    let pid = match get_pid(file_path) {
        Ok(pid) => pid,
        Err(_) => return None,
    };

    super::system().process(pid).map(|_| pid)
}

fn get_pid(file_path: &Path) -> Result<super::Pid, PidError> {
    if let Err(e) = File::open(file_path) {
        return Err(PidError::NoPidFile(e.to_string()));
    }

    match fs::read_to_string(file_path) {
        Ok(content) => {
            let pid_u32 = content
                .trim()
                .parse::<u32>()
                .map_err(|e| PidError::BadPidFile(e.to_string()))?;

            Ok(super::Pid::from_u32(pid_u32))
        }
        Err(e) => Err(PidError::BadPidFile(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_config_yml_output_path_and_content() {
        let _lock = crate::ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();

        let tmp = std::env::temp_dir().join("linkup-test-cf-config");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        unsafe { std::env::set_var("LINKUP_HOME", &tmp) };

        create_config_yml("test-tunnel-id", 9080).unwrap();

        let config_path = tmp.join("cloudflared-config.yml");
        assert!(
            config_path.exists(),
            "config file should be created in LINKUP_HOME"
        );

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            content.contains("http://localhost:9080"),
            "should contain correct port"
        );
        assert!(
            content.contains("test-tunnel-id"),
            "should contain tunnel ID"
        );

        let creds_path = tmp.join("cloudflared-creds.json");
        assert!(
            content.contains(&creds_path.to_string_lossy().to_string()),
            "credentials-file should point to LINKUP_HOME"
        );

        let _ = std::fs::remove_dir_all(&tmp);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }
}
