use std::{
    env,
    fs::File,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    time::Duration,
};

use anyhow::Context;
use reqwest::StatusCode;
use tokio::time::sleep;
use url::Url;

use crate::{
    linkup_certs_dir_path, linkup_file_path,
    state::{upload_state, State},
    worker_client, Result,
};

use super::{BackgroundService, PidError};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] PidError),
    #[error("Failed to reach the local server")]
    ServerUnreachable,
    #[error("WorkerClient error: {0}")]
    WorkerClient(#[from] worker_client::Error),
}

pub struct LocalServer {
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    http_port: u16,
}

impl LocalServer {
    pub fn new(http_port: u16) -> Self {
        Self {
            stdout_file_path: linkup_file_path("localserver-stdout"),
            stderr_file_path: linkup_file_path("localserver-stderr"),
            http_port,
        }
    }

    pub fn url(port: u16) -> Url {
        Url::parse(&format!("http://localhost:{}", port)).expect("linkup url invalid")
    }

    /// Derives HTTPS port from HTTP port (80->443, 9080->9443).
    /// The offset 363 = 443 - 80.
    fn https_port(&self) -> u16 {
        self.http_port.saturating_add(363)
    }

    fn start(&self) -> Result<()> {
        log::debug!("Starting {}", Self::NAME);

        let stdout_file = File::create(&self.stdout_file_path)?;
        let stderr_file = File::create(&self.stderr_file_path)?;

        let mut command = process::Command::new(
            env::current_exe().context("Failed to get the current executable")?,
        );
        command.env(
            "RUST_LOG",
            "info,hickory_server=warn,hyper_util=warn,h2=warn,tower_http=info",
        );
        command.env("LINKUP_SERVICE_ID", super::service_id(Self::ID));
        command.args([
            "server",
            "local-worker",
            "--certs-dir",
            linkup_certs_dir_path().to_str().unwrap(),
            "--http-port",
            &self.http_port.to_string(),
            "--https-port",
            &self.https_port().to_string(),
        ]);

        command
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .spawn()?;

        Ok(())
    }

    async fn reachable(&self) -> bool {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("failed while creating an HTTP client to check readiness of LocalServer");

        let url = format!("{}linkup/check", Self::url(self.http_port));
        let response = client.get(url).send().await;

        matches!(response, Ok(res) if res.status() == StatusCode::OK)
    }

    async fn update_state(&self, state: &mut State) -> Result<()> {
        let session_name = upload_state(state).await?;

        state.linkup.session_name = session_name;
        state
            .save()
            .expect("failed to update local state file with session name");

        Ok(())
    }
}

impl BackgroundService for LocalServer {
    const ID: &str = "linkup-local-server";
    const NAME: &str = "Linkup local server";

    async fn run_with_progress(
        &self,
        state: &mut State,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<()> {
        self.notify_update(&status_sender, super::RunStatus::Starting);

        if self.reachable().await {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Started,
                "Was already running",
            );

            return Ok(());
        }

        if let Err(e) = self.start() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(e);
        }

        let mut reachable = self.reachable().await;
        let mut attempts: u8 = 0;
        loop {
            match (reachable, attempts) {
                (true, _) => break,
                (false, 0..10) => {
                    sleep(Duration::from_millis(1000)).await;
                    attempts += 1;

                    self.notify_update_with_details(
                        &status_sender,
                        super::RunStatus::Starting,
                        format!("Waiting for server... retry #{}", attempts),
                    );

                    reachable = self.reachable().await;
                }
                (false, 10..) => {
                    self.notify_update_with_details(
                        &status_sender,
                        super::RunStatus::Error,
                        "Failed to reach server",
                    );

                    return Err(Error::ServerUnreachable.into());
                }
            }
        }

        match self.update_state(state).await {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_default_port() {
        let url = LocalServer::url(80);
        assert_eq!(url.host_str(), Some("localhost"));
        assert_eq!(url.port_or_known_default(), Some(80));
    }

    #[test]
    fn test_url_custom_port() {
        let url = LocalServer::url(9080);
        assert_eq!(url.as_str(), "http://localhost:9080/");
    }

    #[test]
    fn test_https_port_derivation() {
        let server = LocalServer::new(80);
        assert_eq!(server.https_port(), 443);

        let server = LocalServer::new(9080);
        assert_eq!(server.https_port(), 9443);
    }

    #[test]
    fn test_https_port_max_valid() {
        let server = LocalServer::new(65172);
        assert_eq!(server.https_port(), 65535);
    }

    #[test]
    fn test_https_port_overflow_saturates() {
        let server = LocalServer::new(65535);
        assert_eq!(server.https_port(), u16::MAX);
    }
}
