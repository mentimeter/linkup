use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use crate::{
    linkup_dir_path, linkup_file_path, local_config::LocalState, LINKUP_CF_TLS_API_ENV_VAR,
};

use super::{localserver::LINKUP_LOCAL_SERVER_PORT, BackgroundService};

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

    fn write_caddyfile(&self, domains: &[String]) {
        let mut redis_storage = String::new();

        if let Ok(redis_url) = std::env::var("LINKUP_CERT_STORAGE_REDIS_URL") {
            // This is worth doing to avoid confusion while the redis storage module is new
            if !self.check_redis_installed() {
                // println!("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.");
                // println!("Run `linkup local-dns uninstall && linkup local-dns install`");

                panic!();
            }

            let url = url::Url::parse(&redis_url).unwrap();
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

        if fs::write(&self.caddyfile_path, caddy_template).is_err() {
            panic!(
                "Failed to write Caddyfile at {}",
                &self.caddyfile_path.display(),
            );
        }
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

impl BackgroundService for Caddy {
    fn name(&self) -> String {
        String::from("Caddy")
    }

    fn setup(&self) {}

    fn start(&self) {
        log::debug!("Starting {}", self.name());

        let state = self.state.lock().unwrap();

        if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
            panic!("{} env var is not set", LINKUP_CF_TLS_API_ENV_VAR);
        }

        let domains_and_subdomains: Vec<String> = state
            .domain_strings()
            .iter()
            .map(|domain| format!("{domain}, *.{domain}"))
            .collect();

        self.write_caddyfile(&domains_and_subdomains);

        // Clear previous log file on startup
        fs::write(&self.logfile_path, "").expect(&format!(
            "Failed to clear log file at {}",
            self.logfile_path.display(),
        ));

        Command::new("caddy")
            .current_dir(linkup_dir_path())
            .arg("start")
            .arg("--pidfile")
            .arg(&self.pidfile_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }

    fn ready(&self) -> bool {
        true
    }

    fn update_state(&self) {}

    fn stop(&self) {
        log::debug!("Stopping {}", self.name());

        Command::new("caddy")
            .current_dir(linkup_dir_path())
            .arg("stop")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }

    fn pid(&self) -> Option<String> {
        todo!()
    }
}
