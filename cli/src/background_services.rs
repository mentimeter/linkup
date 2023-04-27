use std::fs::{remove_file, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio, self};
use std::sync::{mpsc, Once};
use std::thread;
use std::time::Duration;

use daemonize::{Child, Daemonize, Outcome};
use regex::Regex;
use thiserror::Error;
use url::Url;

use crate::local_server::local_linkup_main;
use crate::{linkup_file_path, CliError, LINKUP_LOCALSERVER_PID_FILE};
use crate::{LINKUP_CLOUDFLARED_PID, LINKUP_LOCALSERVER_PORT};

const LINKUP_CLOUDFLARED_STDOUT: &str = "cloudflared-stdout";
const LINKUP_CLOUDFLARED_STDERR: &str = "cloudflared-stderr";
const LINKUP_LOCALSERVER_STDOUT: &str = "localserver-stderr";
const LINKUP_LOCALSERVER_STDERR: &str = "localserver-stderr";

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted,
    #[error("cloudflared tunnel not started")]
    TunnelNotStarted,
}

pub fn is_tunnel_started() -> Result<(), CheckErr> {
    if !linkup_file_path(LINKUP_CLOUDFLARED_PID).exists() {
        Err(CheckErr::TunnelNotStarted)
    } else {
        Ok(())
    }
}

pub fn start_tunnel() -> Result<Url, CliError> {
    let stdout_file = File::create(linkup_file_path(LINKUP_CLOUDFLARED_STDOUT)).map_err(|_| {
        CliError::StartLocalTunnel("Failed to create stdout file for local tunnel".to_string())
    })?;
    let stderr_file = File::create(linkup_file_path(LINKUP_CLOUDFLARED_STDERR)).map_err(|_| {
        CliError::StartLocalTunnel("Failed to create stderr file for local tunnel".to_string())
    })?;

    let daemonize = Daemonize::new()
        .pid_file(linkup_file_path(LINKUP_CLOUDFLARED_PID))
        .chown_pid_file(true)
        .working_directory(".")
        .stdout(stdout_file)
        .stderr(stderr_file);

    println!("Starting local tunnel");

    match daemonize.execute() {
        Outcome::Child(child_result) => {
            match child_result {
                Ok(_) => {
                    static ONCE: Once = Once::new();
                    ONCE.call_once(|| {
                        ctrlc::set_handler(move || {
                            let _ = remove_file(linkup_file_path(LINKUP_CLOUDFLARED_PID));
                            std::process::exit(0);
                        })
                        .expect("Failed to set CTRL+C handler");
                    });

                    let child_cmd = Command::new("cloudflared")
                        .args([
                            "tunnel",
                            "--url",
                            &format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT),
                        ])
                        .stdout(Stdio::null())
                        .status();

                    match child_cmd {
                        Ok(_) => {
                            println!("Child process exited successfully");
                            process::exit(0);
                        }
                        Err(e) => {
                            println!("Child process exited with error: {}", e);
                            process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    return Err(CliError::StartLocalTunnel(format!(
                        "Failed to start local tunnel: {}",
                        e
                    )))
                }
            }
        }
        Outcome::Parent(parent_result) => {
            if parent_result.is_err() {
                return Err(CliError::StartLocalTunnel(format!(
                    "Failed to start local tunnel: {}",
                    parent_result.err().unwrap(),
                )));
            }
        }
    }

    let re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com").unwrap();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let stderr_file = File::open(linkup_file_path(LINKUP_CLOUDFLARED_STDERR)).map_err(|_| {
                CliError::StartLocalTunnel("Failed to open stdout file for local tunnel".to_string())
            }).unwrap();

            let buf_reader = BufReader::new(stderr_file);

            for line in buf_reader.lines() {
                let line = line.unwrap_or_default();
                if let Some(mat) = re.find(&line) {
                    let _ = tx.send(Url::parse(mat.as_str()).expect("Failed to parse tunnel URL"));
                    return;
                }
            }

            thread::sleep(Duration::from_millis(100));
        };
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(url) => {
            println!("Tunnel URL: {}", url);
            Ok(url)
        }
        Err(e) => Err(CliError::StartLocalTunnel(
            format!("Failed to obtain tunnel URL within 10 seconds: {}", e),
        )),
    }
}

pub fn is_local_server_started() -> Result<(), CheckErr> {
    if !linkup_file_path(LINKUP_LOCALSERVER_PID_FILE).exists() {
        Err(CheckErr::LocalNotStarted)
    } else {
        Ok(())
    }
}

pub fn start_local_server() -> Result<(), CliError> {
    let stdout_file = File::create(linkup_file_path(LINKUP_LOCALSERVER_STDOUT)).map_err(|_| {
        CliError::StartLocalTunnel("Failed to create stdout file for local server".to_string())
    })?;
    let stderr_file = File::create(linkup_file_path(LINKUP_LOCALSERVER_STDERR)).map_err(|_| {
        CliError::StartLocalTunnel("Failed to create stderr file for local server".to_string())
    })?;

    let daemonize = Daemonize::new()
        .pid_file(linkup_file_path(LINKUP_LOCALSERVER_PID_FILE))
        .chown_pid_file(true)
        .working_directory(".")
        .stdout(stdout_file)
        .stderr(stderr_file)
        .privileged_action(|| {
            static ONCE: Once = Once::new();
            ONCE.call_once(|| {
                ctrlc::set_handler(move || {
                    let _ = remove_file(linkup_file_path(LINKUP_LOCALSERVER_PID_FILE));
                    std::process::exit(0);
                })
                .expect("Failed to set CTRL+C handler");
            });

            match local_linkup_main() {
                Ok(_) => println!("local linkup server finished"),
                Err(e) => println!("local linkup server finished with error {}", e),
            }
        });

    match daemonize.start() {
        Ok(_) => Ok(()),
        Err(e) => Err(CliError::StartLocalServer(format!(
            "Failed to start local server: {}",
            e
        ))),
    }
}
