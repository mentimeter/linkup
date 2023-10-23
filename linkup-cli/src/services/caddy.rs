use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    linkup_dir_path, linkup_file_path, local_config::YamlLocalConfig, CliError, Result,
    LINKUP_CF_TLS_API_ENV_VAR, LINKUP_LOCALSERVER_PORT,
};

const CADDYFILE: &str = "Caddyfile";
const PID_FILE: &str = "caddy-pid";
const LOG_FILE: &str = "caddy-log";

pub fn start(local_config: &YamlLocalConfig) -> Result<()> {
    if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
        return Err(CliError::StartCaddy(format!(
            "{} env var is not set",
            LINKUP_CF_TLS_API_ENV_VAR
        )));
    }

    let domains: Vec<String> = local_config
        .top_level_domains()
        .iter()
        .map(|domain| format!("{domain}, *.{domain}"))
        .collect();

    write_caddyfile(&domains)?;

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

fn write_caddyfile(domains: &[String]) -> Result<()> {
    let caddy_template = format!(
        "
        {{
            http_port 80
            https_port 443
            log {{
                output file {}
            }}
        }}

        {} {{
            reverse_proxy localhost:{}
            tls {{
                dns cloudflare {{env.{}}}
            }}
        }}
        ",
        linkup_file_path(LOG_FILE).display(),
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
