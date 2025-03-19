use std::{
    env,
    fs::File,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    time::Duration,
};

use reqwest::StatusCode;
use tokio::time::sleep;
use url::Url;

use crate::{
    linkup_file_path,
    local_config::{upload_state, LocalState},
    worker_client,
};

use super::{get_running_pid, stop_pid_file, BackgroundService, Pid, PidError, Signal};

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
    pidfile_path: PathBuf,
}

impl LocalServer {
    pub fn new() -> Self {
        Self {
            stdout_file_path: linkup_file_path("localserver-stdout"),
            stderr_file_path: linkup_file_path("localserver-stderr"),
            pidfile_path: linkup_file_path("localserver-pid"),
        }
    }

    pub fn url() -> Url {
        Url::parse("http://localhost").expect("linkup url invalid")
    }

    fn start(&self) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

        let stdout_file = File::create(&self.stdout_file_path)?;
        let stderr_file = File::create(&self.stderr_file_path)?;

        // When running with cargo (e.g. `cargo run -- start`), we should start the server also with cargo.
        let mut command = if env::var("CARGO").is_ok() {
            let mut cmd = process::Command::new("cargo");
            cmd.env("RUST_LOG", "debug");
            cmd.args([
                "run",
                "--",
                "server",
                "--pidfile",
                self.pidfile_path.to_str().unwrap(),
            ]);

            cmd
        } else {
            let mut cmd = process::Command::new("linkup");
            cmd.args(["server", "--pidfile", self.pidfile_path.to_str().unwrap()]);

            cmd
        };

        command
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .spawn()?;

        Ok(())
    }

    pub fn stop(&self) {
        log::debug!("Stopping {}", Self::NAME);

        stop_pid_file(&self.pidfile_path, Signal::Interrupt);
    }

    pub fn running_pid(&self) -> Option<Pid> {
        get_running_pid(&self.pidfile_path)
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

    async fn update_state(&self, state: &mut LocalState) -> Result<(), Error> {
        let session_name = upload_state(state).await?;

        state.linkup.session_name = session_name;
        state
            .save()
            .expect("failed to update local state file with session name");

        Ok(())
    }
}

impl BackgroundService<Error> for LocalServer {
    const NAME: &str = "Linkup local server";

    async fn run_with_progress(
        &self,
        state: &mut LocalState,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
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

                    return Err(Error::ServerUnreachable);
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
