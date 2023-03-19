use std::{path::Path};
use std::fs::remove_file;
use std::sync::Once;

use thiserror::Error;
use actix_web::{web, App, HttpServer, Responder};
use daemonize::Daemonize;

use crate::{CliError, start::get_state, SERPRESS_PID_FILE};


pub fn check() -> Result<(), CliError> {
  let state = get_state()?;

  if let Err(_) = is_local_server_started() {
    start_local_server()?
  }

  Ok(())
}

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted
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
          .bind("127.0.0.1:8080") // You may want to bind to a different address/port.
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
