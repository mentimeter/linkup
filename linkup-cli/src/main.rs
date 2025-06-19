use std::{env, fs, io::ErrorKind, path::PathBuf};

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use colored::Colorize;
use thiserror::Error;

pub use anyhow::Result;
pub use linkup::Version;

use crate::local_config::{config_path, get_config};

mod commands;
mod env_files;
mod local_config;
mod release;
mod services;
mod telemetry;
mod worker_client;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const LINKUP_CONFIG_ENV: &str = "LINKUP_CONFIG";
const LINKUP_DIR: &str = ".linkup";
const LINKUP_STATE_FILE: &str = "state";

pub enum InstallationMethod {
    Brew,
    Cargo,
    Manual,
}

impl InstallationMethod {
    fn current() -> Result<Self> {
        for component in linkup_exe_path()?.components() {
            if component.as_os_str() == "Cellar" {
                return Ok(Self::Brew);
            } else if component.as_os_str() == ".cargo" {
                return Ok(Self::Cargo);
            }
        }

        Ok(Self::Manual)
    }
}

pub fn linkup_exe_path() -> Result<PathBuf> {
    fs::canonicalize(std::env::current_exe().context("Failed to get the current executable")?)
        .context("Failed to canonicalize the executable path")
}

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

pub fn linkup_bin_dir_path() -> PathBuf {
    let mut path = linkup_dir_path();
    path.push("bin");
    path
}

pub fn linkup_certs_dir_path() -> PathBuf {
    let mut path = linkup_dir_path();
    path.push("certs");
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
            _ => Err(anyhow!(
                "Could not create linkup dir at {}: {}",
                path.display(),
                e
            )),
        },
    }
}

fn current_version() -> Version {
    Version::try_from(CURRENT_VERSION)
        .expect("current version on CARGO_PKG_VERSION should be a valid version")
}

fn is_sudo() -> bool {
    let sudo_check = std::process::Command::new("sudo")
        .arg("-n")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .arg("true")
        .status();

    if let Ok(exit_status) = sudo_check {
        return exit_status.success();
    }

    false
}

fn sudo_su() -> Result<()> {
    let status = std::process::Command::new("sudo")
        .arg("su")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if !status.success() {
        return Err(anyhow!("Failed to sudo"));
    }

    Ok(())
}

fn prompt(question: &str) -> String {
    use std::io::Write;

    print!("{}", question);
    std::io::stdout().flush().ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    input
}

async fn display_update_message(command: &Commands) {
    // Cases where we don't want to display the update CLI message.
    match command {
        // We rely on completions output, so we don't want to interfere with it.
        Commands::Completion(_) => return,
        // If the output is json, we don't want to interfere with it.
        Commands::Health(args) if args.json => return,
        // If the output is json, we don't want to interfere with it.
        Commands::Status(args) if args.json => return,
        // Uninstalling, not interested in update.
        Commands::Uninstall(_) => return,
        // Already updating, no reason to show.
        Commands::Update(_) => return,
        _ => (),
    };

    if commands::update::new_version_available().await {
        match commands::update::update_command() {
            Ok(update_command) => {
                let message = format!(
                    "⚠️ New version of linkup is available! Run `{update_command}` to update it.\n"
                )
                .yellow();

                println!("{}", message);
            }
            Err(error) => {
                // TODO(augustoccesar)[2025-03-26]: This should probably be an error log, but for now since the logs
                //   are not behaving the way that we want them to, keep as a warning. Will revisit this once starts
                //   looking into tracing.
                log::warn!("Failed to resolve the update command to display to user: {error}");
            }
        }
    }
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
    about = "Connect remote and local dev/preview environments\n\nIf you need help running linkup, start here:\nhttps://mentimeter.github.io/linkup",
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

    #[clap(about = "Update linkup to the latest released version.")]
    Update(commands::UpdateArgs),

    #[clap(about = "Uninstall linkup and cleanup configurations.")]
    Uninstall(commands::UninstallArgs),

    #[clap(about = "Deploy services to Cloudflare")]
    Deploy(commands::DeployArgs),

    #[clap(about = "Destroy/remove linkup installation from Cloudflare")]
    Destroy(commands::DestroyArgs),

    // Server command is hidden beacuse it is supposed to be managed only by the CLI itself.
    // It is called on `start` to start the local-server.
    #[clap(hide = true)]
    Server(commands::ServerArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config_path = config_path(&cli.config)?;
    let config = get_config(&config_path)?;

    let telemetry =
        telemetry::Telemetry::init(config.linkup.telemetry.map(|telemetry| telemetry.otel));

    ensure_linkup_dir()?;

    display_update_message(&cli.command).await;

    let result = match &cli.command {
        Commands::Health(args) => commands::health(args),
        Commands::Start(args) => commands::start(args, true, &cli.config).await,
        Commands::Stop(args) => commands::stop(args, true),
        Commands::Reset(args) => commands::reset(args).await,
        Commands::Local(args) => commands::local(args).await,
        Commands::Remote(args) => commands::remote(args).await,
        Commands::Status(args) => commands::status(args),
        Commands::LocalDNS(args) => commands::local_dns(args, &cli.config).await,
        Commands::Completion(args) => commands::completion(args),
        Commands::Preview(args) => commands::preview(args, &cli.config).await,
        Commands::Server(args) => commands::server(args).await,
        Commands::Uninstall(args) => commands::uninstall(args, &cli.config).await,
        Commands::Update(args) => commands::update(args).await,
        Commands::Deploy(args) => commands::deploy(args).await,
        Commands::Destroy(args) => commands::destroy(args).await,
    };

    telemetry.shutdown();

    result
}
