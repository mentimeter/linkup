use std::{
    fmt::Write,
    fs::{self, remove_file, File},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, sleep},
    time::{Duration, Instant},
};

use daemonize::{Daemonize, Outcome};
use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    proto::rr::RecordType,
    Resolver,
};
use nix::{libc, sys::signal::Signal};
use regex::Regex;
use reqwest::StatusCode;
use url::Url;

use crate::{
    background_booting::{load_config, ServerConfig},
    linkup_dir_path, linkup_file_path,
    local_config::LocalState,
    services::tunnel,
    signal::send_signal,
    stop::stop_pid_file,
    LINKUP_CF_TLS_API_ENV_VAR,
};

pub trait BackgroundService {
    fn name(&self) -> String;
    fn setup(&self);
    fn start(&self);
    fn ready(&self) -> bool;
    fn update_state(&self);
    fn stop(&self);
    fn pid(&self) -> Option<String>;
}

// ----------------------------------------------------------------
// Local Server
// ----------------------------------------------------------------

const LINKUP_LOCAL_SERVER_PORT: u16 = 9066;

pub struct LocalServer {
    state: Arc<Mutex<LocalState>>,
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
    pidfile_path: PathBuf,
}

impl LocalServer {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            stdout_file_path: linkup_file_path("localserver-stdout"),
            stderr_file_path: linkup_file_path("localserver-stderr"),
            pidfile_path: linkup_file_path("localserver-pid"),
        }
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("http://localhost:{}", LINKUP_LOCAL_SERVER_PORT))
            .expect("linkup url invalid")
    }
}

impl BackgroundService for LocalServer {
    fn name(&self) -> String {
        String::from("Local Linkup server")
    }

    fn setup(&self) {}

    fn start(&self) {
        log::debug!("Starting {}", self.name());

        let stdout_file = File::create(&self.stdout_file_path).unwrap();
        let stderr_file = File::create(&self.stderr_file_path).unwrap();

        let daemonize = Daemonize::new()
            .pid_file(&self.pidfile_path)
            .chown_pid_file(true)
            .working_directory(".")
            .stdout(stdout_file)
            .stderr(stderr_file);

        match daemonize.execute() {
            Outcome::Child(child_result) => match child_result {
                Ok(_) => match linkup_local_server::local_linkup_main() {
                    Ok(_) => {
                        println!("local linkup server finished");
                        process::exit(0);
                    }
                    Err(e) => {
                        println!("local linkup server finished with error {}", e);
                        process::exit(1);
                    }
                },
                Err(e) => todo!(),
            },
            Outcome::Parent(parent_result) => match parent_result {
                Err(e) => todo!(),
                Ok(_) => (),
            },
        }
    }

    fn ready(&self) -> bool {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();

        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(20) {
                return false;
            }

            let response = client.get(self.url()).send();

            if let Ok(resp) = response {
                if resp.status() == StatusCode::OK {
                    return true;
                }
            }

            thread::sleep(Duration::from_millis(2000));
        }
    }

    // TODO(augustoccesar)[2024-12-06]: Revisit this method.
    fn update_state(&self) {
        let mut state = self.state.lock().unwrap();
        let server_config = ServerConfig::from(&*state);

        let server_session_name = load_config(
            &state.linkup.remote,
            &state.linkup.session_name,
            server_config.remote,
        )
        .unwrap();

        let local_session_name =
            load_config(&self.url(), &server_session_name, server_config.local).unwrap();

        if server_session_name != local_session_name {
            // TODO(augustoccesar)[2024-12-06]: Gracefully handle this
            panic!("inconsistent state");
        }

        state.linkup.session_name = server_session_name;
        state.save().unwrap();
    }

    fn stop(&self) {
        log::debug!("Stopping {}", self.name());
        stop_pid_file(&self.pidfile_path, Signal::SIGINT).unwrap();
    }

    fn pid(&self) -> Option<String> {
        if self.pidfile_path.exists() {
            return match fs::read(&self.pidfile_path) {
                Ok(data) => {
                    let pid_str = String::from_utf8(data).unwrap();

                    return if send_signal(&pid_str, Signal::SIGINFO).is_ok() {
                        Some(pid_str.to_string())
                    } else {
                        None
                    };
                }
                Err(_) => None,
            };
        }

        None
    }
}

// ----------------------------------------------------------------
// Cloudlfare tunnel
// ----------------------------------------------------------------

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

        let stderr_content = fs::read_to_string(&self.stderr_file_path).unwrap();

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

    fn setup(&self) {}

    fn start(&self) {
        println!("Starting free tunnel");

        let _ = remove_file(&self.pidfile_path);

        let stdout_file = File::create(&self.stdout_file_path).unwrap();
        let stderr_file = File::create(&self.stderr_file_path).unwrap();

        let url = format!("http://localhost:{}", LINKUP_LOCAL_SERVER_PORT);

        unsafe {
            process::Command::new("cloudflared")
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
                .pre_exec(|| {
                    libc::setsid();

                    Ok(())
                })
                .spawn()
                .unwrap();
        };

        let mut attempts = 0;
        while attempts < 10 && !self.pidfile_path.exists() {
            log::debug!("Waiting for tunnel... attempt {}", attempts + 1);

            sleep(Duration::from_secs(1));
            attempts += 1;
        }
    }

    fn ready(&self) -> bool {
        let state = self.state.lock().unwrap();

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
                    return false;
                }

                let response = resolver.lookup(domain, RecordType::A);

                if let Ok(lookup) = response {
                    let addresses = lookup.iter().collect::<Vec<_>>();

                    if !addresses.is_empty() {
                        log::debug!("DNS has propogated for {}.", domain);
                        thread::sleep(Duration::from_millis(1000));

                        return true;
                    }
                }

                thread::sleep(Duration::from_millis(2000));
            }
        }

        return false;
    }

    fn update_state(&self) {
        let mut state = self.state.lock().unwrap();

        state.linkup.tunnel = Some(self.url());
    }

    fn stop(&self) {
        log::debug!("Stopping {}", self.name());

        stop_pid_file(&self.pidfile_path, Signal::SIGINT).unwrap();
    }

    fn pid(&self) -> Option<String> {
        if self.pidfile_path.exists() {
            return match fs::read(&self.pidfile_path) {
                Ok(data) => {
                    let pid_str = String::from_utf8(data).unwrap();

                    return if send_signal(&pid_str, Signal::SIGINFO).is_ok() {
                        Some(pid_str.to_string())
                    } else {
                        None
                    };
                }
                Err(_) => None,
            };
        }

        None
    }
}

// ----------------------------------------------------------------
// Caddy
// ----------------------------------------------------------------

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
                println!("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.");
                println!("Run `linkup local-dns uninstall && linkup local-dns install`");

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

// ----------------------------------------------------------------
// Dnsmasq
// ----------------------------------------------------------------

pub struct Dnsmasq {
    state: Arc<Mutex<LocalState>>,
    port: u16,
    config_file_path: PathBuf,
    log_file_path: PathBuf,
    pid_file_path: PathBuf,
}

impl Dnsmasq {
    pub fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self {
            state,
            port: 8053,
            config_file_path: linkup_file_path("dnsmasq-conf"),
            log_file_path: linkup_file_path("dnsmasq-log"),
            pid_file_path: linkup_file_path("dnsmasq-pid"),
        }
    }
}

impl BackgroundService for Dnsmasq {
    fn name(&self) -> String {
        String::from("Dnsmasq")
    }

    fn setup(&self) {
        let state = self.state.lock().unwrap();
        let session_name = state.linkup.session_name.clone();

        let local_domains_template =
            state
                .domain_strings()
                .iter()
                .fold(String::new(), |mut acc, d| {
                    let _ = write!(
                        acc,
                        "address=/{}.{}/127.0.0.1\naddress=/{}.{}/::1\nlocal=/{}.{}/\n",
                        session_name, d, session_name, d, session_name, d
                    );
                    acc
                });

        let dnsmasq_template = format!(
            "{}

port={}
log-facility={}
pid-file={}\n",
            local_domains_template,
            self.port,
            self.log_file_path.display(),
            self.pid_file_path.display(),
        );

        if fs::write(&self.config_file_path, dnsmasq_template).is_err() {
            panic!(
                "Failed to write dnsmasq config at {}",
                &self.config_file_path.display()
            );
        }
    }

    fn start(&self) {
        log::debug!("Starting {}", self.name());

        Command::new("dnsmasq")
            .current_dir(linkup_dir_path())
            .arg("--log-queries")
            .arg("-C")
            .arg(&self.config_file_path)
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

        stop_pid_file(&self.pid_file_path, Signal::SIGINT).unwrap();
    }

    fn pid(&self) -> Option<String> {
        todo!()
    }
}

// ----------------------------------------------------------------------

pub fn start_background_services(services: Vec<&dyn BackgroundService>) {
    for service in services {
        service.setup();
        service.start();
        // TODO: Check for ready
        service.update_state();
    }
}

pub fn stop_background_services(services: Vec<&dyn BackgroundService>) {
    for service in services {
        service.stop();
    }
}
