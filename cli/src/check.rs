use std::io::{BufReader, BufRead};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Instant, Duration};
use std::{path::Path};
use std::fs::{remove_file, File};
use std::sync::{Once, mpsc};

use regex::Regex;
use thiserror::Error;
use actix_web::{web, App, HttpServer, Responder};
use daemonize::Daemonize;
use url::Url;

use crate::{SERPRESS_CLOUDFLARED_PID, SERPRESS_PORT};
use crate::start::save_state;
use crate::{CliError, start::get_state, SERPRESS_PID_FILE};

const SERPRESS_CLOUDFLARED_STDOUT: &str = ".serpress-cloudflared-stdout";

// #[derive(Error, Debug)]
// pub enum CliError {
//     #[error("no valid state file: {0}")]
//     NoState(String),
//     #[error("no valid config provided: {0}")]
//     BadConfig(String),
//     #[error("could not save statefile: {0}")]
//     SaveState(String),
//     #[error("could not start local server: {0}")]
//     StartLocalServer(String),
// }

pub fn check() -> Result<(), CliError> {
  let mut state = get_state()?;

  if let Err(_) = is_local_server_started() {
    start_local_server()?
  }

  if let Err(_) = is_tunnel_started() {
    let tunnel = start_tunnel()?;
    state.serpress.tunnel = tunnel;
  }

  save_state(state);
  Ok(())
}

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted,
    #[error("cloudflared tunnel not started")]
    TunnelNotStarted,
}

fn is_tunnel_started() -> Result<(), CheckErr> {
    if !Path::new(SERPRESS_CLOUDFLARED_PID).exists() {
        Err(CheckErr::TunnelNotStarted)
    } else {
        Ok(())
    }
}

fn start_tunnel() -> Result<Url, CliError> {
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
              .args(&["tunnel", "--url", &format!("http://localhost:{}", SERPRESS_PORT)])
              .stdout(Stdio::null())
              .spawn()
              .map_err(|e| CliError::StartLocalTunnel(format!("Failed to run cloudflared tunnel: {}", e)))?;
      }
      Err(e) => return Err(CliError::StartLocalTunnel(format!(
          "Failed to start local tunnel: {}",
          e
      ))),
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

fn is_local_server_started() -> Result<(), CheckErr> {
  if !Path::new(SERPRESS_PID_FILE).exists() {
    Err(CheckErr::LocalNotStarted)
  } else {
      Ok(())
  }
}


// You'll need to replace this with your actual server logic.
async fn server_handler() -> impl Responder {
    "Hello, world!"
}

fn start_local_server() -> Result<(), CliError> {
    let daemonize = Daemonize::new()
        .pid_file(SERPRESS_PID_FILE)
        .chown_pid_file(true)
        .working_directory(".")
        .privileged_action(|| {
          let server = HttpServer::new(|| App::new().route("/", web::get().to(server_handler)))
              .bind(format!("127.0.0.1:{}", SERPRESS_PORT)) // You may want to bind to a different address/port.
              .expect("Failed to bind the server");

          static ONCE: Once = Once::new();
          ONCE.call_once(|| {
              ctrlc::set_handler(move || {
                  let _ = remove_file(SERPRESS_PID_FILE);
                  std::process::exit(0);
              })
              .expect("Failed to set CTRL+C handler");
          });

          // TODO this probably means that the pid file doesn't get deleted if the server crashes
          server.run();
      });

    match daemonize.start() {
        Ok(_) => Ok(()),
        Err(e) => Err(CliError::StartLocalServer(format!(
            "Failed to start local server: {}",
            e
        ))),
    }
}
