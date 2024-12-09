use std::{
    fs::{self, remove_file, File},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
    sync::{Arc, Mutex},
    thread::{self, sleep},
    time::{Duration, Instant},
};

use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    proto::rr::RecordType,
    Resolver,
};
use nix::sys::signal::Signal;
use regex::Regex;
use url::Url;

use crate::{
    linkup_file_path,
    local_config::LocalState,
    signal::{self, get_running_pid},
};

use super::{localserver::LINKUP_LOCAL_SERVER_PORT, stop_pid_file, BackgroundService};

#[derive(thiserror::Error, Debug)]
enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed while locking state file")]
    StateFileLock,
    #[error("Failed to start: {0}")]
    FailedToStart(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
}

pub struct CloudflareTunnel {
    state: Arc<Mutex<LocalState>>,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl CloudflareTunnel {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            stdout_file_path: linkup_file_path("cloudflared-stdout"),
            stderr_file_path: linkup_file_path("cloudflared-stderr"),
            pidfile_path: linkup_file_path("cloudflared-pid"),
        }
    }

    pub fn url(&self) -> Url {
        let tunnel_url_re = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com")
            .expect("Failed to compile regex");

        let stderr_content = fs::read_to_string(&self.stderr_file_path)
            .map_err(|e| Error::from(e))
            .unwrap();

        match tunnel_url_re.find(&stderr_content) {
            Some(url_match) => {
                return Url::parse(url_match.as_str()).expect("Failed to parse tunnel URL");
            }
            None => panic!("failed to find tunnel url"),
        }
    }
}

impl BackgroundService for CloudflareTunnel {
    fn name(&self) -> String {
        String::from("Cloudflare tunnel")
    }

    fn setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = remove_file(&self.pidfile_path);

        let stdout_file = File::create(&self.stdout_file_path).map_err(|e| Error::from(e))?;
        let stderr_file = File::create(&self.stderr_file_path).map_err(|e| Error::from(e))?;

        let url = format!("http://localhost:{}", LINKUP_LOCAL_SERVER_PORT);

        process::Command::new("cloudflared")
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .args(&[
                "tunnel",
                "--url",
                &url,
                "--pidfile",
                self.pidfile_path.to_str().unwrap(),
            ])
            .spawn()?;

        let mut attempts = 0;
        while attempts < 10 && !self.pidfile_path.exists() {
            log::debug!("Waiting for tunnel... attempt {}", attempts + 1);

            sleep(Duration::from_secs(1));
            attempts += 1;
        }

        if self.pidfile_path.exists() {
            Ok(())
        } else {
            Err(Box::new(Error::FailedToStart(
                "Pidfile not found after all atempts of starting exhausted".to_string(),
            )))
        }
    }

    fn ready(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let state = self.state.lock().map_err(|_| Error::StateFileLock)?;

        if let Some(tunnel) = &state.linkup.tunnel {
            log::debug!("Waiting for tunnel DNS to propagate at {}...", tunnel);

            let mut opts = ResolverOpts::default();
            opts.cache_size = 0; // Disable caching

            let resolver = Resolver::new(ResolverConfig::default(), opts).unwrap();

            let start = Instant::now();

            let url = self.url();
            let domain = url.host_str().unwrap();

            loop {
                if start.elapsed() > Duration::from_secs(40) {
                    return Ok(false);
                }

                let response = resolver.lookup(domain, RecordType::A);

                if let Ok(lookup) = response {
                    let addresses = lookup.iter().collect::<Vec<_>>();

                    if !addresses.is_empty() {
                        log::debug!("DNS has propogated for {}.", domain);
                        thread::sleep(Duration::from_millis(1000));

                        return Ok(true);
                    }
                }

                thread::sleep(Duration::from_millis(2000));
            }
        }

        return Ok(false);
    }

    fn update_state(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().map_err(|_| Error::StateFileLock)?;

        state.linkup.tunnel = Some(self.url());

        Ok(())
    }

    fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("Stopping {}", self.name());

        stop_pid_file(&self.pidfile_path, Signal::SIGINT).map_err(|e| Error::from(e))?;

        Ok(())
    }

    fn pid(&self) -> Option<String> {
        get_running_pid(&self.pidfile_path)
    }
}

// fn start_paid_tunnel(
//     sys: &dyn System,
//     paid_manager: &dyn PaidTunnelManager,
//     boot: &dyn BackgroundServices,
//     tunnel_manager: &dyn TunnelManager,
//     mut state: LocalState,
// ) -> Result<(), CliError> {
//     state = boot.boot_linkup_server(state.clone())?;

//     log::info!(
//         "Starting paid tunnel with session name: {}",
//         state.linkup.session_name
//     );
//     let tunnel_name = format!("tunnel-{}", state.linkup.session_name);
//     let mut tunnel_id = match paid_manager.get_tunnel_id(&tunnel_name) {
//         Ok(Some(id)) => id,
//         Ok(None) => "".to_string(),
//         Err(e) => return Err(e),
//     };

//     let mut create_tunnel = false;

//     if tunnel_id.is_empty() {
//         log::info!("Tunnel ID is empty");
//         create_tunnel = true;
//     } else {
//         log::info!("Tunnel ID: {}", tunnel_id);
//         let file_path = format!("{}/.cloudflared/{}.json", sys.get_env("HOME")?, tunnel_id);
//         if sys.file_exists(Path::new(&file_path)) {
//             log::info!("Tunnel config file for {}: {}", tunnel_id, file_path);
//         } else {
//             log::info!("Tunnel config file for {} does not exist", tunnel_id);
//             create_tunnel = true;
//         }
//     }

//     if create_tunnel {
//         println!("Creating tunnel...");
//         tunnel_id = paid_manager.create_tunnel(&tunnel_name)?;
//         paid_manager.create_dns_record(&tunnel_id, &tunnel_name)?;
//     }

//     if tunnel_manager.is_tunnel_running().is_err() {
//         println!("Starting paid tunnel...");
//         state.linkup.tunnel = Some(tunnel_manager.run_tunnel(&state)?);
//     } else {
//         println!("Cloudflare tunnel was already running.. Try stopping linkup first if you have problems.");
//     }
//     state.save()?;

//     if sys.file_exists(&linkup_file_path(LINKUP_LOCALDNS_INSTALL)) {
//         boot.boot_local_dns(state.domain_strings(), state.linkup.session_name.clone())?;
//     }

//     print_session_names(&state);
//     check_local_not_started(&state)?;

//     Ok(())
// }
