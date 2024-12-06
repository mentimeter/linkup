use std::{
    env,
    fs::{self, remove_file, File},
    io::Read,
    os::unix::process::CommandExt,
    process::{self, Stdio},
    sync::Once,
    thread::sleep,
    time::Duration,
};

use nix::{libc, sys::signal::Signal};
use regex::Regex;
use serde::de::Error;
use tunnel::TunnelManager;
use url::Url;

use crate::{
    linkup_file_path, local_config::LocalState, signal::send_signal, stop::stop_pid_file, CliError,
    LINKUP_CLOUDFLARED_PID, LINKUP_LOCALSERVER_PORT,
};

pub mod caddy;
pub mod dnsmasq;
pub mod local_server;
pub mod tunnel;

pub trait BackgroudService<E> {
    fn should_boot(&self) -> bool;
    fn running_pid(&self) -> Option<String>;
    fn healthy(&self) -> bool;
    fn setup(&self) -> Result<(), E>;
    fn start(&self) -> Result<(), E>;
    fn stop(&self) -> Result<(), E>;
}

#[derive(thiserror::Error, Debug)]
pub enum BackgroundServiceError {
    #[error("No tunnel URL found")]
    NoUrlFound,
}

pub struct FreeCloudflareTunnel {
    state: LocalState,
}

impl FreeCloudflareTunnel {
    pub fn load(state: LocalState) -> Self {
        Self { state }
    }

    pub fn tunnel_url(&self) -> Result<Url, BackgroundServiceError> {
        let tunnel_url_re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")
            .expect("Failed to compile regex");

        let stderr_content =
            fs::read_to_string(linkup_file_path(LINKUP_CLOUDFLARED_STDERR)).unwrap();

        match tunnel_url_re.find(&stderr_content) {
            Some(url_match) => {
                return Ok(Url::parse(url_match.as_str()).expect("Failed to parse tunnel URL"));
            }
            None => Err(BackgroundServiceError::NoUrlFound),
        }
    }
}

const LINKUP_CLOUDFLARED_STDOUT: &str = "cloudflared-stdout";
const LINKUP_CLOUDFLARED_STDERR: &str = "cloudflared-stderr";

impl BackgroudService<BackgroundServiceError> for FreeCloudflareTunnel {
    fn should_boot(&self) -> bool {
        !(env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").is_ok()
            && env::var("LINKUP_CLOUDFLARE_ZONE_ID").is_ok()
            && env::var("LINKUP_CF_API_TOKEN").is_ok())
    }

    fn running_pid(&self) -> Option<String> {
        let pidfile = linkup_file_path(LINKUP_CLOUDFLARED_PID);
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

    fn setup(&self) -> Result<(), BackgroundServiceError> {
        Ok(())
    }

    // What will happen with double starts?
    fn start(&self) -> Result<(), BackgroundServiceError> {
        let _ = remove_file(linkup_file_path(LINKUP_CLOUDFLARED_PID));

        let stdout_file = File::create(linkup_file_path(LINKUP_CLOUDFLARED_STDOUT)).unwrap();
        let stderr_file = File::create(linkup_file_path(LINKUP_CLOUDFLARED_STDERR)).unwrap();

        let url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);

        let pidfile = linkup_file_path(LINKUP_CLOUDFLARED_PID);

        unsafe {
            process::Command::new("cloudflared")
                .stdout(stdout_file)
                .stderr(stderr_file)
                .stdin(Stdio::null())
                .args(&[
                    "tunnel",
                    "--url",
                    &url,
                    "--pidfile",
                    pidfile.to_str().unwrap(),
                ])
                .pre_exec(|| {
                    libc::setsid();

                    Ok(())
                })
                .spawn()
                .unwrap();
        };

        let mut attempts = 0;
        while attempts < 10 && !linkup_file_path(LINKUP_CLOUDFLARED_PID).exists() {
            println!("Waiting for tunnel... attempt {}", attempts + 1);

            sleep(Duration::from_secs(1));
            attempts += 1;
        }

        // TODO: Maybe move the state saving to here?
        // self.state.linkup.tunnel = self.tunnel_url().ok();
        // self.state.save();

        Ok(())
    }

    fn stop(&self) -> Result<(), BackgroundServiceError> {
        stop_pid_file(&linkup_file_path(LINKUP_CLOUDFLARED_PID), Signal::SIGINT).unwrap();

        Ok(())
    }

    fn healthy(&self) -> bool {
        todo!()
    }
}
