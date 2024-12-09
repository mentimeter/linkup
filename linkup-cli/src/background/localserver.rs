use std::{
    fs::File,
    path::PathBuf,
    process,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use daemonize::{Daemonize, Outcome};
use nix::sys::signal::Signal;
use reqwest::StatusCode;
use url::Url;

use crate::{
    background_booting::{load_config, ServerConfig},
    linkup_file_path,
    local_config::LocalState,
    signal::{self, get_running_pid},
};

use super::{stop_pid_file, BackgroundService};

pub const LINKUP_LOCAL_SERVER_PORT: u16 = 9066;

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed while locking state file")]
    StateFileLock,
    #[error("Failed to start: {0}")]
    FailedToStart(#[from] daemonize::Error),
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
}

impl BackgroundService for LocalServer {
    fn name(&self) -> String {
        String::from("Local Linkup server")
    }

    fn setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("Starting {}", self.name());

        let stdout_file = File::create(&self.stdout_file_path).map_err(|e| Error::from(e))?;
        let stderr_file = File::create(&self.stderr_file_path).map_err(|e| Error::from(e))?;

        let daemonize = Daemonize::new()
            .pid_file(&self.pidfile_path)
            .chown_pid_file(true)
            .working_directory(".")
            .stdout(stdout_file)
            .stderr(stderr_file);

        match daemonize.execute() {
            Outcome::Child(child_result) => match child_result {
                Ok(_) => match linkup_local_server::local_linkup_main() {
                    Ok(_) => {
                        println!("local linkup server finished");
                        process::exit(0);
                    }
                    Err(e) => {
                        println!("local linkup server finished with error {}", e);
                        process::exit(1);
                    }
                },
                Err(e) => return Err(Box::new(Error::from(e))),
            },
            Outcome::Parent(parent_result) => match parent_result {
                Err(e) => return Err(Box::new(Error::from(e))),
                Ok(_) => (),
            },
        }

        Ok(())
    }

    fn ready(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("failed while creating an HTTP client to check readiness of LocalServer");

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(20) {
                return Ok(false);
            }

            let url = format!("{}linkup-check", self.url());
            let response = client.get(url).send();

            if let Ok(resp) = response {
                if resp.status() == StatusCode::OK {
                    return Ok(true);
                }
            }

            thread::sleep(Duration::from_millis(2000));
        }
    }

    // TODO(augustoccesar)[2024-12-06]: Revisit this method.
    fn update_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().map_err(|_| Error::StateFileLock)?;
        let server_config = ServerConfig::from(&*state);

        // TODO(augustoccesar)[2024-12-09]: Refactor this method to return a different error type.
        let server_session_name = load_config(
            &state.linkup.remote,
            &state.linkup.session_name,
            server_config.remote,
        )
        .expect("failed to load config to get server session name");

        let local_session_name =
            load_config(&self.url(), &server_session_name, server_config.local)
                .expect("failed to load config to get local session name");

        if server_session_name != local_session_name {
            return Err(Box::new(Error::InconsistentState));
        }

        state.linkup.session_name = server_session_name;
        state
            .save()
            .expect("failed to update local state file with session name");

        Ok(())
    }

    fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("Stopping {}", self.name());
        stop_pid_file(&self.pidfile_path, Signal::SIGINT).map_err(|e| Error::from(e))?;

        Ok(())
    }

    fn pid(&self) -> Option<String> {
        get_running_pid(&self.pidfile_path)
    }
}
