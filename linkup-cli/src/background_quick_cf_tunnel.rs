use std::fs::{remove_file, File};
use std::io::{BufRead, BufReader};
use std::process::{self, Command, Stdio};
use std::sync::{mpsc, Once};
use std::thread;
use std::time::Duration;

use daemonize::{Daemonize, Outcome};
use regex::Regex;
use url::Url;

use crate::signal::send_sigint;

use crate::{linkup_file_path, CliError};
use crate::{LINKUP_CLOUDFLARED_PID, LINKUP_LOCALSERVER_PORT};

const LINKUP_CLOUDFLARED_STDOUT: &str = "cloudflared-stdout";
const LINKUP_CLOUDFLARED_STDERR: &str = "cloudflared-stderr";

pub fn start_quick_tunnel() -> Result<Url, CliError> {
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
            Ok(_) => daemonized_tunnel_child(),
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

    let re =
        Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com").expect("Failed to compile regex");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Either the tunnel will start and we'll get a URL, or the propogated error will end the cli command
        loop {
            // TODO consider sync_data instead
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
                        if let Some(mat) = re.find(&line) {
                            let url = Url::parse(mat.as_str()).expect("Failed to parse tunnel URL");
                            tx.send(Ok(url)).expect("Failed to send tunnel URL");
                            return;
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

    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result,
        Err(e) => Err(CliError::StartLocalTunnel(format!(
            "Failed to obtain tunnel URL within 10 seconds: {}",
            e
        ))),
    }
}

fn daemonized_tunnel_child() {
    let mut child_cmd = Command::new("cloudflared")
        .args([
            "tunnel",
            "--url",
            &format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT),
        ])
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
            let kill_res = send_sigint(pid.to_string().as_str());
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
