use std::{env, fs, path::PathBuf, process::Command};

use crate::{
    commands::local_dns, current_version, linkup_bin_dir_path, linkup_file_path,
    local_config::LocalState, release, signal,
};

use super::{local_server::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to start the Caddy service")]
    Starting,
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Missing environment variable '{0}'.")]
    MissingEnvVar(String),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] signal::PidError),
}

#[derive(thiserror::Error, Debug)]
pub enum InstallError {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to fetch release information: {0}")]
    FetchError(#[from] reqwest::Error),
    #[error("Release not found for version {0}")]
    ReleaseNotFound(release::Version),
    #[error("Caddy asset not found on release for version {0}")]
    AssetNotFound(release::Version),
    #[error("Failed to download Caddy asset: {0}")]
    AssetDownload(String),
}

pub struct Caddy {
    caddyfile_path: PathBuf,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl Caddy {
    pub fn new() -> Self {
        Self {
            caddyfile_path: linkup_file_path("Caddyfile"),
            stdout_file_path: linkup_file_path("caddy-stdout"),
            stderr_file_path: linkup_file_path("caddy-stderr"),
            pidfile_path: linkup_file_path("caddy-pid"),
        }
    }

    pub async fn install() -> Result<(), InstallError> {
        let bin_dir_path = linkup_bin_dir_path();
        fs::create_dir_all(&bin_dir_path)?;

        let mut caddy_path = bin_dir_path.clone();
        caddy_path.push("caddy");

        if fs::exists(&caddy_path)? {
            log::debug!(
                "Caddy executable already exists on {}",
                &bin_dir_path.display()
            );
            return Ok(());
        }

        let version = current_version();
        match release::fetch_release(&version).await? {
            Some(release) => {
                let os = env::consts::OS;
                let arch = env::consts::ARCH;

                match release.caddy_asset(os, arch) {
                    Some(asset) => match asset.download_decompressed("caddy").await {
                        Ok(downloaded_caddy_path) => {
                            fs::rename(&downloaded_caddy_path, &caddy_path)?;
                        }
                        Err(error) => return Err(InstallError::AssetDownload(error.to_string())),
                    },
                    None => {
                        log::warn!(
                            "Failed to find Caddy asset on release for version {}",
                            &version
                        );

                        return Err(InstallError::AssetNotFound(version.clone()));
                    }
                }
            }
            None => {
                log::warn!("Failed to find release for version {}", &version);

                return Err(InstallError::ReleaseNotFound(version.clone()));
            }
        }

        Ok(())
    }

    fn start(&self, domains: &[String]) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

        let domains_and_subdomains: Vec<String> = domains
            .iter()
            .map(|domain| format!("{domain}, *.{domain}"))
            .collect();

        self.write_caddyfile(&domains_and_subdomains)?;

        let stdout_file = fs::File::create(&self.stdout_file_path)?;
        let stderr_file = fs::File::create(&self.stderr_file_path)?;

        #[cfg(target_os = "macos")]
        let status = Command::new("./caddy")
            .current_dir(linkup_bin_dir_path())
            .arg("start")
            .arg("--pidfile")
            .arg(&self.pidfile_path)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .status()?;

        #[cfg(target_os = "linux")]
        let status = {
            // To make sure that the local user is the owner of the pidfile and not root,
            // we create it before running the caddy command.
            let _ = fs::File::create(&self.pidfile_path)?;

            Command::new("sudo")
                .current_dir(linkup_bin_dir_path())
                .arg("./caddy")
                .arg("start")
                .arg("--pidfile")
                .arg(&self.pidfile_path)
                .stdin(Stdio::null())
                .stdout(stdout_file)
                .stderr(stderr_file)
                .status()?
        };

        if !status.success() {
            return Err(Error::Starting);
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping {}", Self::NAME);

        signal::stop_pid_file(&self.pidfile_path, signal::Signal::SIGTERM)?;

        Ok(())
    }

    fn write_caddyfile(&self, domains: &[String]) -> Result<(), Error> {
        let cloudflare_kv_config = format!(
            "
            storage cloudflare_kv {{
                api_token       \"{}\"
                account_id      \"{}\"
                namespace_id    \"{}\"
            }}
            ",
            // Presence of these .unwrap() is checked on Caddy#should_start()
            env::var("LINKUP_CLOUDFLARE_API_TOKEN").unwrap(),
            env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID").unwrap(),
            env::var("LINKUP_CLOUDFLARE_KV_NAMESPACE_ID").unwrap()
        );

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
            self.stdout_file_path.display(),
            &cloudflare_kv_config,
            domains.join(", "),
            LINKUP_LOCAL_SERVER_PORT,
            "LINKUP_CLOUDFLARE_API_TOKEN",
        );

        fs::write(&self.caddyfile_path, caddy_template)?;

        Ok(())
    }

    pub fn should_start(&self, domains: &[String]) -> Result<bool, Error> {
        for env_var in [
            "LINKUP_CLOUDFLARE_API_TOKEN",
            "LINKUP_CLOUDFLARE_ACCOUNT_ID",
            "LINKUP_CLOUDFLARE_KV_NAMESPACE_ID",
        ] {
            if env::var(env_var).is_err() {
                return Err(Error::MissingEnvVar(env_var.into()));
            }
        }

        if !is_installed() {
            return Ok(false);
        }

        let resolvers = local_dns::list_resolvers()?;

        Ok(domains.iter().any(|domain| resolvers.contains(domain)))
    }

    pub fn running_pid(&self) -> Option<String> {
        signal::get_running_pid(&self.pidfile_path)
    }
}

impl BackgroundService<Error> for Caddy {
    const NAME: &str = "Caddy";

    async fn run_with_progress(
        &self,
        state: &mut LocalState,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<(), Error> {
        let domains = &state.domain_strings();

        match self.should_start(domains) {
            Ok(true) => (),
            Ok(false) => {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Skipped,
                    "Local DNS not installed",
                );

                return Ok(());
            }
            Err(err) => {
                self.notify_update_with_details(
                    &status_sender,
                    super::RunStatus::Skipped,
                    "Failed to read resolvers folder",
                );

                log::warn!("Failed to read resolvers folder: {}", err);

                return Ok(());
            }
        }

        self.notify_update(&status_sender, super::RunStatus::Starting);

        if self.running_pid().is_some() {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Started,
                "Was already running",
            );

            return Ok(());
        }

        if let Err(e) = self.start(domains) {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(e);
        }

        self.notify_update(&status_sender, super::RunStatus::Started);

        Ok(())
    }
}

pub fn is_installed() -> bool {
    let mut caddy_path = linkup_bin_dir_path();
    caddy_path.push("caddy");

    caddy_path.exists()
}
