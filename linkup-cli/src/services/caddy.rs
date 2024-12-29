use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

use crate::{
    commands::local_dns, linkup_dir_path, linkup_file_path, local_config::LocalState, signal,
    LINKUP_CF_TLS_API_ENV_VAR,
};

use super::{local_server::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Cloudflare TLS API token is required for local-dns Cloudflare TLS certificates.")]
    MissingTlsApiTokenEnv,
    #[error("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.")]
    MissingRedisInstalation,
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

    pub fn install_extra_packages() {
        Command::new("sudo")
            .args(["caddy", "add-package", "github.com/caddy-dns/cloudflare"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();

        Command::new("sudo")
            .args([
                "caddy",
                "add-package",
                "github.com/pberkel/caddy-storage-redis",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }

    fn start(&self, domains: &[String]) -> Result<(), Error> {
        log::debug!("Starting {}", Self::NAME);

        if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
            return Err(Error::MissingTlsApiTokenEnv);
        }

        let domains_and_subdomains: Vec<String> = domains
            .iter()
            .map(|domain| format!("{domain}, *.{domain}"))
            .collect();

        self.write_caddyfile(&domains_and_subdomains)?;

        let stdout_file = fs::File::create(&self.stdout_file_path)?;
        let stderr_file = fs::File::create(&self.stderr_file_path)?;

        Command::new("caddy")
            .current_dir(linkup_dir_path())
            .arg("start")
            .arg("--pidfile")
            .arg(&self.pidfile_path)
            .stdout(stdout_file)
            .stderr(stderr_file)
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
            if !self.check_redis_installed() {
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
            self.stdout_file_path.display(),
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

        output_str.contains("redis")
    }

    fn should_start(&self, domains: &[String]) -> bool {
        let resolvers = local_dns::list_resolvers().unwrap();

        domains.iter().any(|domain| resolvers.contains(domain))
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

        if !self.should_start(domains) {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Skipped,
                "Local DNS not installed",
            );

            return Ok(());
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
    let res = Command::new("which")
        .args(["caddy"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .unwrap();

    res.success()
}
