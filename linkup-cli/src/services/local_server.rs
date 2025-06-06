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
    local_config::{upload_state, LocalState},
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
}

impl LocalServer {
    pub fn new() -> Self {
        Self {
            stdout_file_path: linkup_file_path("localserver-stdout"),
            stderr_file_path: linkup_file_path("localserver-stderr"),
        }
    }

    /// For internal communication to local-server, we only use the port 80 (HTTP).
    pub fn url() -> Url {
        Url::parse("http://localhost:80").expect("linkup url invalid")
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
        command.env("LINKUP_SERVICE_ID", Self::ID);
        command.args([
            "server",
            "local-worker",
            "--certs-dir",
            linkup_certs_dir_path().to_str().unwrap(),
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

        let url = format!("{}linkup/check", Self::url());
        let response = client.get(url).send().await;

        matches!(response, Ok(res) if res.status() == StatusCode::OK)
    }

    async fn update_state(&self, state: &mut LocalState) -> Result<()> {
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
        state: &mut LocalState,
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
