use std::{
    fs::{self, File},
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
    signal::send_signal,
};

use super::{stop_pid_file, BackgroundService};

pub const LINKUP_LOCAL_SERVER_PORT: u16 = 9066;

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

    fn setup(&self) {}

    fn start(&self) {
        log::debug!("Starting {}", self.name());

        let stdout_file = File::create(&self.stdout_file_path).unwrap();
        let stderr_file = File::create(&self.stderr_file_path).unwrap();

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
                Err(e) => panic!("{:?}", e),
            },
            Outcome::Parent(parent_result) => match parent_result {
                Err(e) => panic!("{:?}", e),
                Ok(_) => (),
            },
        }
    }

    fn ready(&self) -> bool {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(20) {
                return false;
            }

            let url = format!("{}linkup-check", self.url());
            let response = client.get(url).send();

            if let Ok(resp) = response {
                if resp.status() == StatusCode::OK {
                    return true;
                }
            }

            thread::sleep(Duration::from_millis(2000));
        }
    }

    // TODO(augustoccesar)[2024-12-06]: Revisit this method.
    fn update_state(&self) {
        let mut state = self.state.lock().unwrap();
        let server_config = ServerConfig::from(&*state);

        let server_session_name = load_config(
            &state.linkup.remote,
            &state.linkup.session_name,
            server_config.remote,
        )
        .unwrap();

        let local_session_name =
            load_config(&self.url(), &server_session_name, server_config.local).unwrap();

        if server_session_name != local_session_name {
            // TODO(augustoccesar)[2024-12-06]: Gracefully handle this
            panic!("inconsistent state");
        }

        state.linkup.session_name = server_session_name;
        state.save().unwrap();
    }

    fn stop(&self) {
        log::debug!("Stopping {}", self.name());
        stop_pid_file(&self.pidfile_path, Signal::SIGINT).unwrap();
    }

    fn pid(&self) -> Option<String> {
        if self.pidfile_path.exists() {
            return match fs::read(&self.pidfile_path) {
                Ok(data) => {
                    let pid_str = String::from_utf8(data).unwrap();

                    return if send_signal(&pid_str, Signal::SIGINFO).is_ok() {
                        Some(pid_str.to_string())
                    } else {
                        None
                    };
                }
                Err(_) => None,
            };
        }

        None
    }
}
