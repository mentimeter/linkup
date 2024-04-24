use std::{env, fs, io::ErrorKind, path::PathBuf};

use clap::{builder::ValueParser, Parser, Subcommand};
use clap_complete::Shell;
use thiserror::Error;

mod background_booting;
mod completion;
mod env_files;
mod local_config;
mod local_dns;
mod preview;
mod remote_local;
mod reset;
mod services;
mod signal;
mod start;
mod status;
mod stop;
mod worker_client;

use completion::completion;
use preview::preview;
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
const LINKUP_LOCALDNS_INSTALL: &str = "localdns-install";
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
    #[error("could not stop: {0}")]
    StopErr(String),
    #[error("could not get status: {0}")]
    StatusErr(String),
    #[error("your session is in an inconsistent state. Stop your session before trying again.")]
    InconsistentState,
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
}

#[derive(Error, Debug)]
pub enum CheckErr {
    #[error("local server not started")]
    LocalNotStarted,
    #[error("cloudflared tunnel not started")]
    TunnelNotStarted,
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
enum LocalDNSSubcommand {
    Install,
    Uninstall,
}

#[derive(Subcommand)]
enum Commands {
    #[clap(about = "Start a new linkup session")]
    Start {
        #[clap(
            short,
            long,
            help = "Start linkup in partial mode without a tunnel. Not all requests will succeed."
        )]
        no_tunnel: bool,
    },

    #[clap(about = "Stop a running linkup session")]
    Stop,

    #[clap(about = "Reset a linkup session")]
    Reset,

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

    #[clap(about = "Speed up your local environment by routing traffic locally when possible")]
    LocalDNS {
        #[clap(subcommand)]
        subcommand: LocalDNSSubcommand,
    },

    #[clap(about = "Generate completions for your shell")]
    Completion {
        #[arg(long, value_enum)]
        shell: Option<Shell>,
    },

    #[clap(about = "Create a \"permanent\" Linkup preview")]
    Preview {
        #[arg(
            help = "<service>=<url> pairs to preview.",
            value_parser = ValueParser::new(preview::parse_services_tuple),
            required = true,
            num_args = 1..,
        )]
        services: Vec<(String, String)>,

        #[arg(long, help = "Print the request body instead of sending it.")]
        print_request: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    ensure_linkup_dir()?;

    match &cli.command {
        Commands::Start { no_tunnel } => start(&cli.config, *no_tunnel),
        Commands::Stop => stop(),
        Commands::Reset => reset(),
        Commands::Local { service_names } => local(service_names),
        Commands::Remote { service_names } => remote(service_names),
        Commands::Status { json, all } => status(*json, *all),
        Commands::LocalDNS { subcommand } => match subcommand {
            LocalDNSSubcommand::Install => local_dns::install(&cli.config),
            LocalDNSSubcommand::Uninstall => local_dns::uninstall(&cli.config),
        },
        Commands::Completion { shell } => completion(shell),
        Commands::Preview {
            services,
            print_request,
        } => preview(&cli.config, services, *print_request),
    }
}
