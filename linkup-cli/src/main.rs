use std::{env, fs, path::PathBuf};

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use colored::Colorize;
use thiserror::Error;

pub use anyhow::Result;
pub use linkup::Version;

mod commands;
mod env_files;
mod release;
mod services;
mod state;
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

/// Resolves the linkup state directory. Precedence:
/// 1. `LINKUP_HOME` env var (explicit override)
/// 2. `LINKUP_HOME` in a `.env` file found by walking up from the current directory
/// 3. `~/.linkup/active-instance` file (set by `linkup instance-use`)
/// 4. `~/.linkup` (default)
///
/// In cases 1 and 2, `LINKUP_CONFIG` is also resolved from `.env` so the correct
/// worktree config is used even when a stale value is inherited from the shell.
pub fn linkup_dir_path() -> PathBuf {
    if let Ok(home) = env::var("LINKUP_HOME") {
        if !home.is_empty() {
            set_linkup_config_from_dotenv();
            return PathBuf::from(home);
        }
    }

    if let Some(home) = linkup_home_from_dotenv() {
        return PathBuf::from(home);
    }

    if let Some(active) = commands::instance_use::active_instance_dir() {
        return active;
    }

    let storage_dir = match env::var("HOME") {
        Ok(val) => val,
        Err(_e) => "/var/tmp".to_string(),
    };

    let mut path = PathBuf::new();
    path.push(storage_dir);
    path.push(LINKUP_DIR);
    path
}

/// Walk up from the current directory looking for a `.env` file containing `LINKUP_HOME=...`.
/// Also sets `LINKUP_CONFIG` via `set_linkup_config_from_dotenv`.
fn linkup_home_from_dotenv() -> Option<String> {
    let home = read_dotenv_var("LINKUP_HOME")?;
    set_linkup_config_from_dotenv();
    Some(home)
}

/// Walk up from the current directory looking for a `.env` file containing `LINKUP_CONFIG=...`
/// and override the process env var with it. This ensures the worktree-specific config is used
/// even when a stale `LINKUP_CONFIG` is inherited from a parent shell.
fn set_linkup_config_from_dotenv() {
    if let Some(cfg) = read_dotenv_var("LINKUP_CONFIG") {
        env::set_var(LINKUP_CONFIG_ENV, cfg);
    }
}

/// Walk up from the current directory looking for a `.env` file containing `key=value`.
/// Returns the first non-empty value found, or `None`.
fn read_dotenv_var(key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    let mut dir = env::current_dir().ok()?;
    loop {
        let env_file = dir.join(".env");
        if env_file.is_file() {
            if let Ok(content) = fs::read_to_string(&env_file) {
                for line in content.lines() {
                    if let Some(value) = line.strip_prefix(&prefix) {
                        let value = value.trim();
                        if !value.is_empty() {
                            return Some(value.to_string());
                        }
                    }
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub fn default_linkup_dir_path() -> PathBuf {
    let storage_dir = match env::var("HOME") {
        Ok(val) => val,
        Err(_e) => "/var/tmp".to_string(),
    };

    let mut path = PathBuf::new();
    path.push(storage_dir);
    path.push(LINKUP_DIR);
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

    match fs::create_dir_all(&path) {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow!(
            "Could not create linkup dir at {}: {}",
            path.display(),
            e
        )),
    }
}

#[cfg(test)]
pub(crate) static ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        vars: Vec<(String, Option<String>)>,
        prev_dir: Option<std::path::PathBuf>,
    }

    impl EnvGuard {
        fn new(vars: &[&str]) -> Self {
            let saved = vars
                .iter()
                .map(|k| (k.to_string(), env::var(k).ok()))
                .collect();
            Self {
                vars: saved,
                prev_dir: env::current_dir().ok(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, val) in &self.vars {
                match val {
                    Some(v) => unsafe { env::set_var(key, v) },
                    None => unsafe { env::remove_var(key) },
                }
            }
            if let Some(dir) = &self.prev_dir {
                let _ = env::set_current_dir(dir);
            }
        }
    }

    #[test]
    fn test_ensure_linkup_dir_creates_nested_dirs() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let prev = std::env::var("LINKUP_HOME").ok();

        let tmp = std::env::temp_dir().join("linkup-test-ensure-dir");
        let nested = tmp.join("a").join("b").join("c");
        let _ = fs::remove_dir_all(&tmp);

        unsafe { std::env::set_var("LINKUP_HOME", &nested) };

        let result = ensure_linkup_dir();
        assert!(
            result.is_ok(),
            "ensure_linkup_dir should create nested dirs"
        );
        assert!(nested.exists());

        // Idempotent
        let result2 = ensure_linkup_dir();
        assert!(result2.is_ok(), "ensure_linkup_dir should be idempotent");

        let _ = fs::remove_dir_all(&tmp);

        if let Some(val) = prev {
            unsafe { std::env::set_var("LINKUP_HOME", val) };
        } else {
            unsafe { std::env::remove_var("LINKUP_HOME") };
        }
    }

    #[test]
    fn test_dotenv_finds_linkup_home_and_config() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvGuard::new(&["LINKUP_HOME", LINKUP_CONFIG_ENV]);
        unsafe {
            env::remove_var("LINKUP_HOME");
            env::remove_var(LINKUP_CONFIG_ENV);
        }

        let tmp = env::temp_dir().join("linkup-test-dotenv-home-config");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join(".env"),
            "OTHER_VAR=foo\nLINKUP_HOME=/tmp/test-instance\nLINKUP_CONFIG=/tmp/worktree.yaml\n",
        )
        .unwrap();
        env::set_current_dir(&tmp).unwrap();

        let home = linkup_home_from_dotenv();
        assert_eq!(home.as_deref(), Some("/tmp/test-instance"));
        assert_eq!(
            env::var(LINKUP_CONFIG_ENV).ok().as_deref(),
            Some("/tmp/worktree.yaml")
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_dotenv_config_overrides_inherited() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvGuard::new(&["LINKUP_HOME", LINKUP_CONFIG_ENV]);

        unsafe {
            env::remove_var("LINKUP_HOME");
            env::set_var(LINKUP_CONFIG_ENV, "/stale/main-worktree/linkup-config.yaml");
        }

        let tmp = env::temp_dir().join("linkup-test-dotenv-override");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join(".env"),
            "LINKUP_CONFIG=/correct/worktree/linkup-config.worktree.yaml\n",
        )
        .unwrap();
        env::set_current_dir(&tmp).unwrap();

        set_linkup_config_from_dotenv();

        assert_eq!(
            env::var(LINKUP_CONFIG_ENV).ok().as_deref(),
            Some("/correct/worktree/linkup-config.worktree.yaml"),
            "dotenv LINKUP_CONFIG should override inherited value"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_dotenv_walks_up_directories() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvGuard::new(&["LINKUP_HOME", LINKUP_CONFIG_ENV]);
        unsafe {
            env::remove_var("LINKUP_HOME");
            env::remove_var(LINKUP_CONFIG_ENV);
        }

        let tmp = env::temp_dir().join("linkup-test-dotenv-walk");
        let nested = tmp.join("applications").join("editor");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            tmp.join(".env"),
            "LINKUP_HOME=/tmp/walk-instance\nLINKUP_CONFIG=/tmp/walk-config.yaml\n",
        )
        .unwrap();
        env::set_current_dir(&nested).unwrap();

        let home = linkup_home_from_dotenv();
        assert_eq!(
            home.as_deref(),
            Some("/tmp/walk-instance"),
            "should find LINKUP_HOME in ancestor .env"
        );
        assert_eq!(
            env::var(LINKUP_CONFIG_ENV).ok().as_deref(),
            Some("/tmp/walk-config.yaml"),
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_linkup_dir_path_with_env_var_also_sets_config() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvGuard::new(&["LINKUP_HOME", LINKUP_CONFIG_ENV]);

        let tmp = env::temp_dir().join("linkup-test-dir-path-config");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join(".env"),
            "LINKUP_CONFIG=/correct/worktree-config.yaml\n",
        )
        .unwrap();
        env::set_current_dir(&tmp).unwrap();

        unsafe {
            env::set_var("LINKUP_HOME", "/explicit/instance");
            env::set_var(LINKUP_CONFIG_ENV, "/stale/main-config.yaml");
        }

        let result = linkup_dir_path();
        assert_eq!(result, PathBuf::from("/explicit/instance"));
        assert_eq!(
            env::var(LINKUP_CONFIG_ENV).ok().as_deref(),
            Some("/correct/worktree-config.yaml"),
            "linkup_dir_path should correct LINKUP_CONFIG from .env even when LINKUP_HOME is set via env var"
        );

        let _ = fs::remove_dir_all(&tmp);
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

    #[clap(about = "Remove a specific linkup instance by number")]
    InstanceRemove(commands::InstanceRemoveArgs),

    #[clap(about = "Remove all linkup instances (except the default)")]
    InstanceRemoveAll(commands::InstanceRemoveAllArgs),

    #[clap(about = "Switch to a linkup instance")]
    InstanceUse(commands::InstanceUseArgs),

    // Server command is hidden beacuse it is supposed to be managed only by the CLI itself.
    // It is called on `start` to start the local-server.
    #[clap(hide = true)]
    Server(commands::ServerArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    ensure_linkup_dir()?;

    display_update_message(&cli.command).await;

    match &cli.command {
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
        Commands::InstanceRemove(args) => commands::instance_remove(args).await,
        Commands::InstanceRemoveAll(args) => commands::instance_remove_all(args).await,
        Commands::InstanceUse(args) => commands::instance_use(args),
    }
}
