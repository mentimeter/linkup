use std::{env, fs, io::ErrorKind, path::PathBuf};

use clap::{Parser, Subcommand};
use thiserror::Error;

mod background_booting;
mod background_local_server;
mod background_tunnel;
mod local_config;
mod local_server;
mod remote_local;
mod reset;
mod signal;
mod start;
mod status;
mod stop;

use remote_local::{local, remote};
use reset::reset;
use start::start;
use status::status;
use stop::stop;

const LINKUP_CONFIG_ENV: &str = "LINKUP_CONFIG";
const LINKUP_LOCALSERVER_PORT: u16 = 9066;
const LINKUP_DIR: &str = ".linkup";
const LINKUP_STATE_FILE: &str = "state";
const LINKUP_LOCALSERVER_PID_FILE: &str = "localserver-pid";
const LINKUP_CLOUDFLARED_PID: &str = "cloudflared-pid";
const LINKUP_ENV_SEPARATOR: &str = "##### Linkup environment - DO NOT EDIT #####";

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

#[derive(Error, Debug)]
pub enum CliError {
    #[error("no valid state file: {0}")]
    NoState(String),
    #[error("there was a problem with the provided config: {0}")]
    BadConfig(String),
    #[error("no valid config file provided: {0}")]
    NoConfig(String),
    #[error("a service directory was provided that contained no .env.*.linkup file: {0}")]
    NoDevEnv(String),
    #[error("couldn't set env for service {0}: {1}")]
    SetServiceEnv(String, String),
    #[error("couldn't remove env for service {0}: {1}")]
    RemoveServiceEnv(String, String),
    #[error("could not save statefile: {0}")]
    SaveState(String),
    #[error("could not start local server: {0}")]
    StartLocalServer(String),
    #[error("could not start local tunnel: {0}")]
    StartLocalTunnel(String),
    #[error("linkup component did not start in time: {0}")]
    StartLinkupTimeout(String),
    #[error("could not load config to {0}: {1}")]
    LoadConfig(String, String),
    #[error("could not stop: {0}")]
    StopErr(String),
    #[error("could not get status: {0}")]
    StatusErr(String),
    #[error("your session is in an inconsistent state. Stop your session before trying again.")]
    InconsistentState,
    #[error("no such service: {0}")]
    NoSuchService(String),
}

#[derive(Parser)]
#[command(
    name = "linkup",
    about = "Connect remote and local dev/preview environments"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Start a new linkup session")]
    Start {
        #[arg(short, long)]
        config: Option<String>,
    },
    #[clap(about = "Stop a running linkup session")]
    Stop {},
    #[clap(about = "Reset a linkup session")]
    Reset {
        #[arg(short, long)]
        config: Option<String>,
    },
    #[clap(about = "Route session traffic to a local service")]
    Local { service_names: Vec<String> },
    #[clap(about = "Route session traffic to a remote service")]
    Remote { service_names: Vec<String> },
    #[clap(about = "View linkup component and service status")]
    Status {
        // Output status in JSON format
        #[arg(long)]
        json: bool,
        #[arg(short, long)]
        all: bool,
    },
}

fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    ensure_linkup_dir()?;

    match &cli.command {
        Commands::Start { config } => start(config.clone()),
        Commands::Stop {} => stop(),
        Commands::Reset { config } => reset(config.clone()),
        Commands::Local { service_names } => local(service_names.clone()),
        Commands::Remote { service_names } => remote(service_names.clone()),
        Commands::Status { json, all } => status(*json, *all),
    }
}
