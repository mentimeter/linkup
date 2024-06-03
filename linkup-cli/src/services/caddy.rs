use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    linkup_dir_path, linkup_file_path, CliError, Result, LINKUP_CF_TLS_API_ENV_VAR,
    LINKUP_LOCALSERVER_PORT,
};

const CADDYFILE: &str = "Caddyfile";
const PID_FILE: &str = "caddy-pid";
const LOG_FILE: &str = "caddy-log";

pub fn start(domains: Vec<String>) -> Result<()> {
    if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
        return Err(CliError::StartCaddy(format!(
            "{} env var is not set",
            LINKUP_CF_TLS_API_ENV_VAR
        )));
    }

    let domains_and_subdomains: Vec<String> = domains
        .iter()
        .map(|domain| format!("{domain}, *.{domain}"))
        .collect();

    write_caddyfile(&domains_and_subdomains)?;

    // Clear previous log file on startup
    fs::write(linkup_file_path(LOG_FILE), "").map_err(|err| {
        CliError::WriteFile(format!(
            "Failed to clear log file at {}, error: {}",
            linkup_file_path(LOG_FILE).display(),
            err
        ))
    })?;

    Command::new("caddy")
        .current_dir(linkup_dir_path())
        .arg("start")
        .arg("--pidfile")
        .arg(linkup_file_path(PID_FILE))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StartCaddy(err.to_string()))?;

    Ok(())
}

pub fn stop() -> Result<()> {
    Command::new("caddy")
        .current_dir(linkup_dir_path())
        .arg("stop")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StopErr(err.to_string()))?;

    Ok(())
}

pub fn install_cloudflare_package() -> Result<()> {
    Command::new("caddy")
        .args(["add-package", "github.com/caddy-dns/cloudflare"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StartCaddy(err.to_string()))?;

    Ok(())
}

pub fn install_redis_package() -> Result<()> {
    Command::new("caddy")
        .args(["add-package", "github.com/pberkel/caddy-storage-redis"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StartCaddy(err.to_string()))?;

    Ok(())
}

fn write_caddyfile(domains: &[String]) -> Result<()> {
    let mut redis_storage = String::new();

    if let Ok(redis_url) = std::env::var("LINKUP_CERT_STORAGE_REDIS_URL") {
        // This is worth doing to avoid confusion while the redis storage module is new
        check_redis_installed()?;

        let url = url::Url::parse(&redis_url)
            .map_err(|_| CliError::StartCaddy(format!("Invalid REDIS_URL: {}", redis_url)))?;
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
        linkup_file_path(LOG_FILE).display(),
        redis_storage,
        domains.join(", "),
        LINKUP_LOCALSERVER_PORT,
        LINKUP_CF_TLS_API_ENV_VAR,
    );

    let caddyfile_path = linkup_file_path(CADDYFILE);
    if fs::write(&caddyfile_path, caddy_template).is_err() {
        return Err(CliError::WriteFile(format!(
            "Failed to write Caddyfile at {}",
            caddyfile_path.display(),
        )));
    }

    Ok(())
}

fn check_redis_installed() -> Result<()> {
    let output = Command::new("caddy")
        .arg("list-modules")
        .output()
        .map_err(|err| CliError::StartCaddy(err.to_string()))?;

    let output_str = String::from_utf8(output.stdout).map_err(|_| {
        CliError::StartCaddy("Failed to parse caddy list-modules output".to_string())
    })?;

    if !output_str.contains("redis") {
        println!("Redis shared storage is a new feature! You need to uninstall and reinstall local-dns to use it.");
        println!("Run `linkup local-dns uninstall && linkup local-dns install`");

        return Err(CliError::StartCaddy("Redis module not found".to_string()));
    }

    Ok(())
}
