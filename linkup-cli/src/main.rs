use std::{env, fs, io::ErrorKind, path::PathBuf, process};

use clap::{Parser, Subcommand};
use thiserror::Error;

mod commands;
mod env_files;
mod local_config;
mod services;
mod signal;
mod system;
mod worker_client;

const LINKUP_CONFIG_ENV: &str = "LINKUP_CONFIG";
const LINKUP_LOCALSERVER_PORT: u16 = 9066;
const LINKUP_DIR: &str = ".linkup";
const LINKUP_STATE_FILE: &str = "state";
const LINKUP_CF_TLS_API_ENV_VAR: &str = "LINKUP_CF_API_TOKEN";

pub fn linkup_dir_path() -> PathBuf {
    let storage_dir = match env::var("HOME") {
        Ok(val) => val,
        Err(_e) => "/var/tmp".to_string(),
    };

    let mut path = PathBuf::new();
    path.push(storage_dir);
    path.push(LINKUP_DIR);
    path
}

pub fn linkup_file_path(file: &str) -> PathBuf {
    let mut path = linkup_dir_path();
    path.push(file);
    path
}

fn ensure_linkup_dir() -> Result<()> {
    let path = linkup_dir_path();

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

fn is_sudo() -> bool {
    let sudo_check = process::Command::new("sudo")
        .arg("-n")
        .arg("true")
        .status();

    if let Ok(exit_status) = sudo_check {
        return exit_status.success();
    }

    false
}

pub type Result<T> = std::result::Result<T, CliError>;

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
    #[error("could not start Caddy: {0}")]
    StartCaddy(String),
    #[error("could not start DNSMasq: {0}")]
    StartDNSMasq(String),
    #[error("could not load config to {0}: {1}")]
    LoadConfig(String, String),
    #[error("could not start: {0}")]
    StartErr(String),
    #[error("could not stop: {0}")]
    StopErr(String),
    #[error("could not get status: {0}")]
    StatusErr(String),
    #[error("no such service: {0}")]
    NoSuchService(String),
    #[error("failed to install local dns: {0}")]
    LocalDNSInstall(String),
    #[error("failed to uninstall local dns: {0}")]
    LocalDNSUninstall(String),
    #[error("failed to write file: {0}")]
    WriteFile(String),
    #[error("failed to reboot dnsmasq: {0}")]
    RebootDNSMasq(String),
    #[error("--no-tunnel does not work without `local-dns`")]
    NoTunnelWithoutLocalDns,
    #[error("could not get env var: {0}")]
    GetEnvVar(String),
    #[error("HTTP error: {0}")]
    HttpErr(String),
    #[error("could not parse: {0}. {1}")]
    ParseErr(String, String),
    #[error("{0}: {1}")]
    FileErr(String, String),
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    WorkerClientErr(#[from] worker_client::Error),
}

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted,
    #[error("cloudflared tunnel not started")]
    TunnelNotRunning,
}

#[derive(Parser)]
#[command(
    name = "linkup",
    about = "Connect remote and local dev/preview environments\n\nIf you need help running linkup, start here:\nhttps://github.com/mentimeter/linkup/blob/main/docs/using-linkup.md",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Cli {
    #[arg(
        short,
        long,
        value_name = "CONFIG",
        help = "Path to config file, overriding environment variable."
    )]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Output the health of the CLI service")]
    Health(commands::HealthArgs),

    #[clap(about = "Start a new linkup session")]
    Start(commands::StartArgs),

    #[clap(about = "Stop a running linkup session")]
    Stop(commands::StopArgs),

    #[clap(about = "Reset a linkup session")]
    Reset(commands::ResetArgs),

    #[clap(about = "Route session traffic to a local service")]
    Local(commands::LocalArgs),

    #[clap(about = "Route session traffic to a remote service")]
    Remote(commands::RemoteArgs),

    #[clap(about = "View linkup component and service status")]
    Status(commands::StatusArgs),

    #[clap(about = "Speed up your local environment by routing traffic locally when possible")]
    LocalDNS(commands::LocalDnsArgs),

    #[clap(about = "Generate completions for your shell")]
    Completion(commands::CompletionArgs),

    #[clap(about = "Create a \"permanent\" Linkup preview")]
    Preview(commands::PreviewArgs),

    // Server command is hidden beacuse it is supposed to be managed only by the CLI itself.
    // It is called on `start` to start the local-server.
    #[clap(hide = true)]
    Server(commands::ServerArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    ensure_linkup_dir()?;

    match &cli.command {
        Commands::Health(args) => commands::health(args),
        Commands::Start(args) => commands::start(args, true, &cli.config).await,
        Commands::Stop(args) => commands::stop(args, true),
        Commands::Reset(args) => commands::reset(args).await,
        Commands::Local(args) => commands::local(args).await,
        Commands::Remote(args) => commands::remote(args).await,
        Commands::Status(args) => commands::status(args),
        Commands::LocalDNS(args) => commands::local_dns(args, &cli.config),
        Commands::Completion(args) => commands::completion(args),
        Commands::Preview(args) => commands::preview(args, &cli.config).await,
        Commands::Server(args) => commands::server(args).await,
    }
}
