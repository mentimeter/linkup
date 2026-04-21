use std::{
    env,
    fs::{self, File},
    os::unix::process::CommandExt,
    path::Path,
    process::{self, Command, Stdio},
    time::Duration,
};

use hickory_resolver::{TokioResolver, config::ResolverOpts, proto::rr::RecordType};
use log::debug;
use serde::{Deserialize, Serialize};
use sysinfo::Pid;
use tokio::time::sleep;
use url::Url;

use linkup::TunnelData;

use super::{PidError, ServiceId};
use crate::{Result, linkup_file_path, state::State};

const ID: ServiceId = ServiceId("cloudflare-tunnel");

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] PidError),
    #[error("Failed to find tunnel URL")]
    UrlNotFound,
    #[error("Failed to find pidfile")]
    PidfileNotFound,
    #[error("Failed to verify that DNS got propagated")]
    DNSNotPropagated,
    #[error("Invalid session name: '{0}'")]
    InvalidSessionName(String),
}

pub fn is_installed() -> bool {
    let res = Command::new("which")
        .args(["cloudflared"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .unwrap();

    res.success()
}

pub async fn start(state: &mut State, tunnel_data: &TunnelData) -> Result<()> {
    let pidfile_path = linkup_file_path("cloudflared-pid");
    let tunnel_url = Url::parse(&tunnel_data.url).expect("tunnel_data url to be valid URL");

    if !state.should_use_tunnel() {
        log::info!("Skipping. State file requested no tunnel.");

        return Ok(());
    }

    if super::find_pid(ID).is_some() {
        log::info!("Already running. Skipping starting tunnel.");

        return Ok(());
    }

    log::info!("Starting...");
    spawn_process(tunnel_data, &pidfile_path).await?;

    // Pidfile existence check
    {
        let mut pidfile_ready_attempt = 0;
        let mut pidfile_exists = get_running_pid(&pidfile_path).is_some();
        while !pidfile_exists && pidfile_ready_attempt <= 10 {
            sleep(Duration::from_secs(1)).await;
            pidfile_ready_attempt += 1;

            log::info!("Waiting for tunnel... retry #{pidfile_ready_attempt}");

            pidfile_exists = get_running_pid(&pidfile_path).is_some();
        }

        if !pidfile_exists {
            log::error!("Failed to start tunnel");

            return Err(Error::PidfileNotFound.into());
        }
    }

    // DNS Propagation check
    {
        let mut dns_propagation_attempt = 0;
        let mut dns_propagated = has_dns_propagated(&tunnel_url).await;

        while !dns_propagated && dns_propagation_attempt <= 20 {
            sleep(Duration::from_secs(2)).await;
            dns_propagation_attempt += 1;

            log::info!("Waiting for tunnel DNS to propagate... retry #{dns_propagation_attempt}");

            dns_propagated = has_dns_propagated(&tunnel_url).await;
        }

        if !dns_propagated {
            log::error!("Failed to propagate tunnel DNS");

            return Err(Error::DNSNotPropagated.into());
        }
    }

    match update_state(state, &tunnel_url) {
        Ok(_) => {
            log::info!("Started");
        }
        Err(e) => {
            log::error!("Failed to start");

            return Err(e);
        }
    }

    Ok(())
}

pub fn stop() {
    super::stop(ID);
}

pub fn find_pid() -> Option<Pid> {
    super::find_pid(ID)
}

async fn spawn_process(tunnel_data: &TunnelData, pidfile_path: &Path) -> Result<()> {
    let stdout_file = File::create(linkup_file_path("cloudflared-stdout"))?;
    let stderr_file = File::create(linkup_file_path("cloudflared-stderr"))?;

    save_tunnel_credentials(
        &tunnel_data.account_id,
        &tunnel_data.id,
        &tunnel_data.secret,
    )?;
    create_config_yml(&tunnel_data.id)?;

    log::debug!("Starting tunnel with name: {}", tunnel_data.name);

    process::Command::new("cloudflared")
        .process_group(0)
        .stdout(stdout_file)
        .stderr(stderr_file)
        .stdin(Stdio::null())
        .env("LINKUP_SERVICE_ID", ID.to_string())
        .args([
            "tunnel",
            "--pidfile",
            pidfile_path
                .to_str()
                .expect("pidfile path to be valid UTF-8"),
            "run",
        ])
        .spawn()?;

    Ok(())
}

async fn has_dns_propagated(tunnel_url: &Url) -> bool {
    let mut opts = ResolverOpts::default();
    opts.cache_size = 0; // Disable caching

    let resolver = TokioResolver::builder_tokio()
        .expect("TokioResolver to be buildable")
        .with_options(opts)
        .build()
        .expect("TokioResolver to be buildable from ResolverOpts");

    let domain = tunnel_url.host_str().unwrap();

    let response = resolver.lookup(domain, RecordType::A).await;

    if let Ok(lookup) = response {
        let addresses = lookup.answers().iter().collect::<Vec<_>>();

        if !addresses.is_empty() {
            log::debug!("DNS has propogated for {}.", domain);

            return true;
        }
    } else {
        log::debug!("DNS {} not propagated yet.", domain);
    }

    false
}

fn update_state(state: &mut State, tunnel_url: &Url) -> Result<()> {
    debug!("Adding tunnel url {} to the state", tunnel_url.as_str());

    state.linkup.tunnel = Some(tunnel_url.clone());
    state
        .save()
        .expect("failed to update local state file with tunnel url");

    Ok(())
}

fn save_tunnel_credentials(
    account_id: &str,
    tunnel_id: &str,
    tunnel_secret: &str,
) -> Result<(), Error> {
    let data = serde_json::json!({
        "AccountTag": account_id,
        "TunnelID": tunnel_id,
        "TunnelSecret": tunnel_secret,
    });

    let home_dir = env::var("HOME").expect("HOME environment variable to be present");
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let file_path = dir_path.join(format!("{}.json", tunnel_id));

    fs::write(&file_path, data.to_string())?;

    Ok(())
}

fn create_config_yml(tunnel_id: &str) -> Result<(), Error> {
    #[derive(Serialize, Deserialize)]
    struct Config {
        url: String,
        tunnel: String,
        #[serde(rename = "credentials-file")]
        credentials_file: String,
    }

    let home_dir = env::var("HOME").expect("HOME environment variable to be present");
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
    }

    let credentials_file_path = dir_path.join(format!("{}.json", tunnel_id));
    let credentials_file_path_str = credentials_file_path.to_string_lossy().to_string();

    let config = Config {
        url: "http://localhost".to_string(),
        tunnel: tunnel_id.to_string(),
        credentials_file: credentials_file_path_str,
    };

    let serialized = serde_yaml::to_string(&config).expect("Failed to serialize config");

    fs::write(dir_path.join("config.yml"), serialized)?;

    Ok(())
}

// Get the pid from a pidfile, but only return Some in case the pidfile is valid and the written pid on the file
// is running.
fn get_running_pid(file_path: &Path) -> Option<super::Pid> {
    let pid = match get_pid(file_path) {
        Ok(pid) => pid,
        Err(_) => return None,
    };

    super::system().process(pid).map(|_| pid)
}

fn get_pid(file_path: &Path) -> Result<super::Pid, PidError> {
    if let Err(e) = File::open(file_path) {
        return Err(PidError::NoPidFile(e.to_string()));
    }

    match fs::read_to_string(file_path) {
        Ok(content) => {
            let pid_u32 = content
                .trim()
                .parse::<u32>()
                .map_err(|e| PidError::BadPidFile(e.to_string()))?;

            Ok(super::Pid::from_u32(pid_u32))
        }
        Err(e) => Err(PidError::BadPidFile(e.to_string())),
    }
}
