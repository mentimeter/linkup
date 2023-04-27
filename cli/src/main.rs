use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use thiserror::Error;

mod background_services;
mod check;
mod local_config;
mod local_server;
mod start;

use start::start;

const LINKUP_CONFIG_ENV: &str = "LINKUP_CONFIG";
const LINKUP_LOCALSERVER_PORT: u16 = 9066;
const LINKUP_DIR: &str = ".linkup";
const LINKUP_STATE_FILE: &str = "state";
const LINKUP_LOCALSERVER_PID_FILE: &str = "local-server-pid";
const LINKUP_CLOUDFLARED_PID: &str = "cloudflared-pid";

pub fn linkup_file_path(file: &str) -> PathBuf {
    let storage_dir = match env::var("HOME") {
        Ok(val) => val,
        Err(_e) => "/var/tmp".to_string(),
    };

    let mut path = PathBuf::new();
    path.push(storage_dir);
    path.push(LINKUP_DIR);
    path.push(file);
    path
}

fn ensure_linkup_dir() -> Result<(), CliError> {
    let storage_dir = match env::var("HOME") {
        Ok(val) => val,
        Err(_e) => "/var/tmp".to_string(),
    };

    let mut path = PathBuf::new();
    path.push(storage_dir);
    path.push(LINKUP_DIR);

    match fs::create_dir(&path) {
        Ok(_) => Ok(()),
        Err(e) => match e.kind() {
            ErrorKind::AlreadyExists => Ok(()),
            _ => Err(CliError::BadConfig(format!(
                "Could not create linkup dir at {}: {}",
                path.display(),
                e
            ))),
        },
    }
}

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Error, Debug)]
pub enum CliError {
    #[error("no valid state file: {0}")]
    NoState(String),
    #[error("no valid config provided: {0}")]
    BadConfig(String),
    #[error("could not save statefile: {0}")]
    SaveState(String),
    #[error("could not start local server: {0}")]
    StartLocalServer(String),
    #[error("could not start local tunnel: {0}")]
    StartLocalTunnel(String),
    #[error("could not load config to {0}: {1}")]
    LoadConfig(String, String),
    #[error("your session is in an inconsistent state. Stop your session before trying again.")]
    InconsistentState,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long)]
        config: Option<String>,
    },
    Stop {},
    Check {},
    Local {},
    Remote {},
}

fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    ensure_linkup_dir()?;

    match &cli.command {
       Commands::Start{config}=> {
            start(config.clone())
       },
       Commands::Stop{} => {
        println!("Stop");
        Err(CliError::BadConfig(String::from("no good")))
       }
       Commands::Check{} => {
        println!("Check");
        Err(CliError::BadConfig(String::from("no good")))
       }
       Commands::Local{} =>{
         println!("Local");
        Err(CliError::BadConfig(String::from("no good")))
       }
       Commands::Remote{} => {
        println!("Remote");
        Err(CliError::BadConfig(String::from("no good")))
       }

    //    _Stop => println!("Stop"),
    //    _Check => println!("Check"),
    //    _Local => println!("Local"),
    }
}
