use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use crate::{
    linkup_dir_path, linkup_file_path, local_config::LocalState, LINKUP_CF_TLS_API_ENV_VAR,
};

use super::{local_server::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Something went wrong...")] // TODO: Remove Default variant for specific ones
    Default,
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed while locking state file")]
    StateFileLock,
    #[error("Missing Cloudflare TLS API token on the environment variables")]
    MissingTlsApiTokenEnv,
    #[error("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.")]
    MissingRedisInstalation,
}

pub struct Caddy {
    state: Arc<Mutex<LocalState>>,
    caddyfile_path: PathBuf,
    logfile_path: PathBuf,
    pidfile_path: PathBuf,
}

impl Caddy {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            caddyfile_path: linkup_file_path("Caddyfile"),
            logfile_path: linkup_file_path("caddy-log"),
            pidfile_path: linkup_file_path("caddy-pid"),
        }
    }

    pub fn install_extra_packages() {
        Command::new("caddy")
            .args(["add-package", "github.com/caddy-dns/cloudflare"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();

        Command::new("caddy")
            .args(["add-package", "github.com/pberkel/caddy-storage-redis"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }

    fn start(&self) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

        let state = self.state.lock().map_err(|_| Error::StateFileLock)?;

        if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
            return Err(Error::MissingTlsApiTokenEnv);
        }

        let domains_and_subdomains: Vec<String> = state
            .domain_strings()
            .iter()
            .map(|domain| format!("{domain}, *.{domain}"))
            .collect();

        self.write_caddyfile(&domains_and_subdomains)?;

        // Clear previous log file on startup
        fs::write(&self.logfile_path, "")?;

        Command::new("caddy")
            .current_dir(linkup_dir_path())
            .arg("start")
            .arg("--pidfile")
            .arg(&self.pidfile_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;

        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);

        Command::new("caddy")
            .current_dir(linkup_dir_path())
            .arg("stop")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;

        Ok(())
    }

    fn write_caddyfile(&self, domains: &[String]) -> Result<(), Error> {
        let mut redis_storage = String::new();

        if let Ok(redis_url) = std::env::var("LINKUP_CERT_STORAGE_REDIS_URL") {
            // This is worth doing to avoid confusion while the redis storage module is new
            if !self.check_redis_installed() {
                // println!("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.");
                // println!("Run `linkup local-dns uninstall && linkup local-dns install`");

                return Err(Error::MissingRedisInstalation);
            }

            let url = url::Url::parse(&redis_url).expect("failed to parse Redis URL");
            redis_storage = format!(
                "
                storage redis {{
                    host           {}
                    port           {}
                    username       \"{}\"
                    password       \"{}\"
                    key_prefix     \"caddy\"
                    compression    true
                }}
                ",
                url.host().unwrap(),
                url.port().unwrap_or(6379),
                url.username(),
                url.password().unwrap(),
            );
        }

        let caddy_template = format!(
            "
            {{
                http_port 80
                https_port 443
                log {{
                    output file {}
                }}
                {}
            }}
    
            {} {{
                reverse_proxy localhost:{}
                tls {{
                    dns cloudflare {{env.{}}}
                }}
            }}
            ",
            self.logfile_path.display(),
            redis_storage,
            domains.join(", "),
            LINKUP_LOCAL_SERVER_PORT,
            LINKUP_CF_TLS_API_ENV_VAR,
        );

        fs::write(&self.caddyfile_path, caddy_template)?;

        Ok(())
    }

    fn check_redis_installed(&self) -> bool {
        let output = Command::new("caddy").arg("list-modules").output().unwrap();

        let output_str = String::from_utf8(output.stdout).unwrap();

        if !output_str.contains("redis") {
            return false;
        }

        return true;
    }
}

impl BackgroundService<Error> for Caddy {
    const NAME: &str = "Caddy";

    async fn run_with_progress(
        &self,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        self.notify_update(&status_sender, super::RunStatus::Starting);

        if let Err(_) = self.start() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(Error::Default);
        }

        self.notify_update(&status_sender, super::RunStatus::Started);

        Ok(())
    }
}
