use std::fs::{remove_file, File};
use std::io::{BufRead, BufReader};
use std::process::{self, Command, Stdio};
use std::sync::{mpsc, Once};
use std::thread;
use std::time::Duration;

use daemonize::{Daemonize, Outcome};
use nix::sys::signal::Signal;
use regex::Regex;
use url::Url;

use crate::local_config::LocalState;
use crate::signal::send_signal;

use crate::stop::stop_pid_file;
use crate::{linkup_file_path, CheckErr, CliError};
use crate::{LINKUP_CLOUDFLARED_PID, LINKUP_LOCALSERVER_PORT};

const LINKUP_CLOUDFLARED_STDOUT: &str = "cloudflared-stdout";
const LINKUP_CLOUDFLARED_STDERR: &str = "cloudflared-stderr";

const TUNNEL_START_WAIT: u64 = 20;

#[cfg_attr(test, mockall::automock)]
pub trait TunnelManager {
    fn run_tunnel(&self, state: &LocalState) -> Result<Url, CliError>;
    fn is_tunnel_running(&self) -> Result<(), CheckErr>;
}

pub struct RealTunnelManager;

impl TunnelManager for RealTunnelManager {
    fn run_tunnel(&self, state: &LocalState) -> Result<Url, CliError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match try_run_tunnel(state) {
                Ok(url) => return Ok(url),
                Err(CliError::StopErr(e)) => {
                    return Err(CliError::StopErr(format!(
                        "Failed to stop tunnel when retrying tunnel boot: {}",
                        e
                    )))
                }
                Err(err) => {
                    println!("Tunnel failed to boot within the time limit. Retrying...");
                    if attempt >= 3 {
                        return Err(err);
                    }
                    // Give the tunnel a chance to clean up
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
    fn is_tunnel_running(&self) -> Result<(), CheckErr> {
        if linkup_file_path(LINKUP_CLOUDFLARED_PID).exists() {
            Ok(())
        } else {
            Err(CheckErr::TunnelNotRunning)
        }
    }
}

fn try_run_tunnel(state: &LocalState) -> Result<Url, CliError> {
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

    match daemonize.execute() {
        Outcome::Child(child_result) => match child_result {
            Ok(_) => daemonized_tunnel_child(state),
            Err(e) => {
                return Err(CliError::StartLocalTunnel(format!(
                    "Failed to start local tunnel: {}",
                    e
                )))
            }
        },
        Outcome::Parent(parent_result) => match parent_result {
            Ok(_) => (),
            Err(error) => {
                return Err(CliError::StartLocalTunnel(format!(
                    "Failed to start local tunnel: {}",
                    error
                )))
            }
        },
    }

    let is_paid = state.linkup.is_paid.is_some() && state.linkup.is_paid.unwrap();
    let session_name = state.linkup.session_name.clone();

    let tunnel_url_re =
        Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com").expect("Failed to compile regex");
    let tunnel_started_re =
        Regex::new(r"Registered tunnel connection").expect("Failed to compile regex");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut url = None;
        let mut found_started = false;

        loop {
            let stderr_file =
                File::open(linkup_file_path(LINKUP_CLOUDFLARED_STDERR)).map_err(|_| {
                    CliError::StartLocalTunnel(
                        "Failed to open stdout file for local tunnel".to_string(),
                    )
                });

            match stderr_file {
                Ok(file) => {
                    let buf_reader: BufReader<File> = BufReader::new(file);

                    for line in buf_reader.lines() {
                        let line = line.unwrap_or_default();
                        if is_paid {
                            url = Some(
                                Url::parse(
                                    format!("https://tunnel-{}.mentimeter.dev", session_name)
                                        .as_str(),
                                )
                                .expect("Failed to parse tunnel URL"),
                            );
                        } else if let Some(url_match) = tunnel_url_re.find(&line) {
                            let found_url =
                                Url::parse(url_match.as_str()).expect("Failed to parse tunnel URL");
                            url = Some(found_url);
                        }

                        if let Some(_started_match) = tunnel_started_re.find(&line) {
                            found_started = true;
                        }

                        if found_started {
                            if let Some(url) = &url {
                                tx.send(Ok(url.clone())).expect("Failed to send tunnel URL");
                                return;
                            }
                        }
                    }
                }
                Err(err) => {
                    tx.send(Err(err)).expect("Failed to send stderr_file error");
                }
            };

            thread::sleep(Duration::from_millis(100));
        }
    });
    match rx.recv_timeout(Duration::from_secs(TUNNEL_START_WAIT)) {
        Ok(result) => result,
        Err(e) => {
            stop_pid_file(&linkup_file_path(LINKUP_CLOUDFLARED_PID), Signal::SIGINT)?;
            Err(CliError::StartLocalTunnel(format!(
                "Failed to obtain tunnel URL within {} seconds: {}",
                TUNNEL_START_WAIT, e
            )))
        }
    }
}

fn daemonized_tunnel_child(state: &LocalState) {
    let url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);
    let is_paid = state.linkup.is_paid.is_some() && state.linkup.is_paid.unwrap();
    let cmd_args: Vec<&str> = match is_paid {
        true => vec!["tunnel", "run", state.linkup.session_name.as_str()],
        false => vec!["tunnel", "--url", url.as_str()],
    };
    log::info!("Starting cloudflared tunnel with args: {:?}", cmd_args);
    let mut child_cmd = Command::new("cloudflared")
        .args(cmd_args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Failed to start cloudflared tunnel");

    let pid = child_cmd.id();
    println!("Tunnel child process started {}", pid);

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        ctrlc::set_handler(move || {
            println!("Killing child process {}", pid);
            let kill_res = send_signal(pid.to_string().as_str(), Signal::SIGINT);
            println!("Kill result: {:?}", kill_res);

            let _ = remove_file(linkup_file_path(LINKUP_CLOUDFLARED_PID));
            std::process::exit(0);
        })
        .expect("Failed to set CTRL+C handler");
    });

    println!("Awaiting child tunnel process exit");
    let status = child_cmd.wait();

    match status {
        Ok(exit_status) => {
            println!("Child process exited with status {}", exit_status);
            process::exit(0);
        }
        Err(e) => {
            println!("Child process exited with error: {}", e);
            process::exit(1);
        }
    }
}
