use std::{
    fs::File,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use reqwest::StatusCode;
use url::Url;

use crate::{
    background_booting::{load_config, ServerConfig},
    linkup_file_path,
    local_config::LocalState,
    signal,
};

use super::BackgroundService;

pub const LINKUP_LOCAL_SERVER_PORT: u16 = 9066;

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
    #[error("Local and remote servers have inconsistent state")]
    InconsistentState,
}

pub struct LocalServer {
    state: Arc<Mutex<LocalState>>,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl LocalServer {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            stdout_file_path: linkup_file_path("localserver-stdout"),
            stderr_file_path: linkup_file_path("localserver-stderr"),
            pidfile_path: linkup_file_path("localserver-pid"),
        }
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("http://localhost:{}", LINKUP_LOCAL_SERVER_PORT))
            .expect("linkup url invalid")
    }

    fn start(&self) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

        let stdout_file = File::create(&self.stdout_file_path).map_err(|e| Error::from(e))?;
        let stderr_file = File::create(&self.stderr_file_path).map_err(|e| Error::from(e))?;

        process::Command::new("linkup")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .args(&["server", "--pidfile", self.pidfile_path.to_str().unwrap()])
            .spawn()?;

        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);
        signal::stop_pid_file(&self.pidfile_path, signal::Signal::SIGINT)
            .map_err(|e| Error::from(e))?;

        Ok(())
    }

    async fn reachable(&self) -> bool {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("failed while creating an HTTP client to check readiness of LocalServer");

        let url = format!("{}linkup-check", self.url());
        let response = client.get(url).send().await;

        match response {
            Ok(res) if res.status() == StatusCode::OK => true,
            _ => false,
        }
    }

    // TODO(augustoccesar)[2024-12-06]: Revisit this method.
    async fn update_state(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::StateFileLock)?;
        let server_config = ServerConfig::from(&*state);

        // TODO(augustoccesar)[2024-12-09]: Refactor this method to return a different error type.
        let server_session_name = load_config(
            &state.linkup.remote,
            &state.linkup.session_name,
            server_config.remote,
        )
        .await
        .expect("failed to load config to get server session name");

        let local_session_name =
            load_config(&self.url(), &server_session_name, server_config.local)
                .await
                .expect("failed to load config to get local session name");

        if server_session_name != local_session_name {
            return Err(Error::InconsistentState);
        }

        state.linkup.session_name = server_session_name;
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

        let mut reachable = self.reachable().await;
        let mut attempts: u8 = 0;
        loop {
            match (reachable, attempts) {
                (true, _) => break,
                (false, 0..10) => {
                    thread::sleep(Duration::from_millis(1000));
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

                    return Err(Error::Default);
                }
            }
        }

        match self.update_state().await {
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
