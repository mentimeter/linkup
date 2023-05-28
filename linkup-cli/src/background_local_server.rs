use std::fs::{remove_file, File};
use std::process::{self};
use std::sync::Once;

use daemonize::{Daemonize, Outcome};
use thiserror::Error;

use crate::local_server::local_linkup_main;
use crate::LINKUP_CLOUDFLARED_PID;
use crate::{linkup_file_path, CliError, LINKUP_LOCALSERVER_PID_FILE};

const LINKUP_LOCALSERVER_STDOUT: &str = "localserver-stdout";
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
        });

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
            Err(e) => Err(CliError::StartLocalTunnel(format!(
                "Failed to start local server: {}",
                e
            ))),
        },
        Outcome::Parent(parent_result) => match parent_result {
            Err(e) => Err(CliError::StartLocalTunnel(format!(
                "Failed to start local server: {}",
                e,
            ))),
            Ok(_) => Ok(()),
        },
    }
}
