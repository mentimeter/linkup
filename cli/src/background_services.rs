use std::fs::{remove_file, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Once};
use std::thread;
use std::time::Duration;

use daemonize::Daemonize;
use regex::Regex;
use thiserror::Error;
use url::Url;

use crate::local_server::local_serpress_main;
use crate::{CliError, SERPRESS_PID_FILE};
use crate::{SERPRESS_CLOUDFLARED_PID, SERPRESS_PORT};

const SERPRESS_CLOUDFLARED_STDOUT: &str = ".serpress-cloudflared-stdout";

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted,
    #[error("cloudflared tunnel not started")]
    TunnelNotStarted,
}

pub fn is_tunnel_started() -> Result<(), CheckErr> {
    if !Path::new(SERPRESS_CLOUDFLARED_PID).exists() {
        Err(CheckErr::TunnelNotStarted)
    } else {
        Ok(())
    }
}

pub fn start_tunnel() -> Result<Url, CliError> {
    let stdout_file = File::create(SERPRESS_CLOUDFLARED_STDOUT).map_err(|_| {
        CliError::StartLocalTunnel("Failed to create stdout file for local tunnel".to_string())
    })?;

    let daemonize = Daemonize::new()
        .pid_file(SERPRESS_CLOUDFLARED_PID)
        .chown_pid_file(true)
        .working_directory(".")
        .stdout(stdout_file);

    match daemonize.start() {
        Ok(_) => {
            static ONCE: Once = Once::new();
            ONCE.call_once(|| {
                ctrlc::set_handler(move || {
                    let _ = remove_file(SERPRESS_CLOUDFLARED_PID);
                    std::process::exit(0);
                })
                .expect("Failed to set CTRL+C handler");
            });

            Command::new("cloudflared")
                .args(&[
                    "tunnel",
                    "--url",
                    &format!("http://localhost:{}", SERPRESS_PORT),
                ])
                .stdout(Stdio::null())
                .spawn()
                .map_err(|e| {
                    CliError::StartLocalTunnel(format!("Failed to run cloudflared tunnel: {}", e))
                })?;
        }
        Err(e) => {
            return Err(CliError::StartLocalTunnel(format!(
                "Failed to start local tunnel: {}",
                e
            )))
        }
    }

    let stdout_file = File::open(SERPRESS_CLOUDFLARED_STDOUT).map_err(|_| {
        CliError::StartLocalTunnel("Failed to open stdout file for local tunnel".to_string())
    })?;

    let re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com").unwrap();
    let buf_reader = BufReader::new(stdout_file);

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in buf_reader.lines() {
            let line = line.unwrap_or_default();
            if let Some(mat) = re.find(&line) {
                let _ = tx.send(Url::parse(mat.as_str()).expect("Failed to parse tunnel URL"));
                return;
            }
        }
    });

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(url) => Ok(url),
        Err(_) => Err(CliError::StartLocalTunnel(
            "Failed to obtain tunnel URL within 10 seconds".to_string(),
        )),
    }
}

pub fn is_local_server_started() -> Result<(), CheckErr> {
    if !Path::new(SERPRESS_PID_FILE).exists() {
        Err(CheckErr::LocalNotStarted)
    } else {
        Ok(())
    }
}

pub fn start_local_server() -> Result<(), CliError> {
    let daemonize = Daemonize::new()
        .pid_file(SERPRESS_PID_FILE)
        .chown_pid_file(true)
        .working_directory(".")
        .privileged_action(|| {
            static ONCE: Once = Once::new();
            ONCE.call_once(|| {
                ctrlc::set_handler(move || {
                    let _ = remove_file(SERPRESS_PID_FILE);
                    std::process::exit(0);
                })
                .expect("Failed to set CTRL+C handler");
            });

            match local_serpress_main() {
                Ok(_) => println!("local serpress server finished"),
                Err(e) => println!(
                    "local serpress server finished with error {}",
                    e.to_string()
                ),
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
