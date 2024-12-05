use std::fs::{self, File};
use std::process::{self};
use std::thread;
use std::time::{Duration, Instant};

use daemonize::{Daemonize, Outcome};
use linkup_local_server::local_linkup_main;
use nix::sys::signal::Signal;
use reqwest::StatusCode;
use url::Url;

use crate::background_booting::{load_config, ServerConfig};
use crate::linkup_file_path;
use crate::local_config::LocalState;
use crate::signal::send_signal;
use crate::stop::stop_pid_file;

use super::{BackgroudService, BackgroundServiceError};

const LINKUP_LOCALSERVER_STDOUT: &str = "localserver-stdout";
const LINKUP_LOCALSERVER_STDERR: &str = "localserver-stderr";
const LINKUP_LOCALSERVER_PID_FILE: &str = "localserver-pid";
const LINKUP_LOCALSERVER_PORT: u16 = 9066;

pub struct LinkupServer {}

impl LinkupServer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
            .expect("linkup url invalid")
    }

    pub fn update_state(&self, mut state: LocalState) -> LocalState {
        let server_config = ServerConfig::from(&state);

        let server_session_name = load_config(
            &state.linkup.remote,
            &state.linkup.session_name,
            server_config.remote,
        )
        .unwrap();

        let local_session_name =
            load_config(&self.url(), &server_session_name, server_config.local).unwrap();

        if server_session_name != local_session_name {
            panic!("ayo?");
            // return Err(CliError::InconsistentState);
        }

        state.linkup.session_name = server_session_name;
        state.save().unwrap();

        state
    }
}

impl BackgroudService<BackgroundServiceError> for LinkupServer {
    fn name(&self) -> String {
        String::from("Linkup local server")
    }

    fn should_boot(&self) -> bool {
        true
    }

    fn running_pid(&self) -> Option<String> {
        let pidfile = linkup_file_path(LINKUP_LOCALSERVER_PID_FILE);
        if pidfile.exists() {
            return match fs::read(pidfile) {
                Ok(data) => {
                    let pid_str = String::from_utf8(data).unwrap();

                    return if send_signal(&pid_str, None).is_ok() {
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

    fn healthy(&self) -> bool {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();

        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(20) {
                return false;
            }

            let response = client.get(self.url()).send();

            if let Ok(resp) = response {
                if resp.status() == StatusCode::OK {
                    return true;
                }
            }

            thread::sleep(Duration::from_millis(300));
        }
    }

    fn setup(&self) -> Result<(), BackgroundServiceError> {
        Ok(())
    }

    fn start(&self) -> Result<(), BackgroundServiceError> {
        let stdout_file = File::create(linkup_file_path(LINKUP_LOCALSERVER_STDOUT)).unwrap();
        let stderr_file = File::create(linkup_file_path(LINKUP_LOCALSERVER_STDERR)).unwrap();

        let daemonize = Daemonize::new()
            .pid_file(linkup_file_path(LINKUP_LOCALSERVER_PID_FILE))
            .chown_pid_file(true)
            .working_directory(".")
            .stdout(stdout_file)
            .stderr(stderr_file);

        match daemonize.execute() {
            Outcome::Child(child_result) => match child_result {
                Ok(_) => match local_linkup_main() {
                    Ok(_) => {
                        println!("local linkup server finished");
                        process::exit(0);
                    }
                    Err(e) => {
                        println!("local linkup server finished with error {}", e);
                        process::exit(1);
                    }
                },
                Err(e) => todo!(),
            },
            Outcome::Parent(parent_result) => match parent_result {
                Err(e) => todo!(),
                Ok(_) => Ok(()),
            },
        }
    }

    fn stop(&self) -> Result<(), BackgroundServiceError> {
        stop_pid_file(
            &linkup_file_path(LINKUP_LOCALSERVER_PID_FILE),
            Signal::SIGINT,
        )
        .unwrap();

        Ok(())
    }
}
