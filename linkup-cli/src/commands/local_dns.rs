use std::{
    fs,
    process::{Command, Stdio},
};

use clap::Subcommand;

use crate::{
    is_sudo,
    local_config::{config_path, get_config},
    services, sudo_su, CliError, Result, LINKUP_CF_TLS_API_ENV_VAR,
};

#[derive(clap::Args)]
pub struct Args {
    #[clap(subcommand)]
    pub subcommand: LocalDNSSubcommand,
}

#[derive(Subcommand)]
pub enum LocalDNSSubcommand {
    Install,
    Uninstall,
}

pub fn local_dns(args: &Args, config: &Option<String>) -> Result<()> {
    match args.subcommand {
        LocalDNSSubcommand::Install => install(config),
        LocalDNSSubcommand::Uninstall => uninstall(config),
    }
}

pub fn install(config_arg: &Option<String>) -> Result<()> {
    if std::env::var(LINKUP_CF_TLS_API_ENV_VAR).is_err() {
        println!("local-dns uses Cloudflare to enable https through local certificates.");
        println!(
            "To use it, you need to set the {} environment variable.",
            LINKUP_CF_TLS_API_ENV_VAR
        );
        return Err(CliError::LocalDNSInstall(format!(
            "{} env var is not set",
            LINKUP_CF_TLS_API_ENV_VAR
        )));
    }

    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Ensure there is a folder /etc/resolvers");
        println!("  - Create file(s) for /etc/resolver/<domain>");
        println!("  - Flush DNS cache");

        sudo_su()?;
    }

    ensure_resolver_dir()?;
    install_resolvers(&input_config.top_level_domains())?;

    println!("Installing extra caddy packages, this could take a while...");
    services::Caddy::install_extra_packages();

    Ok(())
}

pub fn uninstall(config_arg: &Option<String>) -> Result<()> {
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Delete file(s) on /etc/resolver");
        println!("  - Flush DNS cache");
    }

    uninstall_resolvers(&input_config.top_level_domains())?;

    Ok(())
}

fn ensure_resolver_dir() -> Result<()> {
    Command::new("sudo")
        .args(["mkdir", "/etc/resolver"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| {
            CliError::LocalDNSInstall(format!(
                "failed to create /etc/resolver folder. Reason: {}",
                err
            ))
        })?;

    Ok(())
}

fn install_resolvers(resolve_domains: &[String]) -> Result<()> {
    for domain in resolve_domains.iter() {
        let cmd_str = format!(
            "echo \"nameserver 127.0.0.1\nport 8053\" > /etc/resolver/{}",
            domain
        );
        let status = Command::new("sudo")
            .arg("bash")
            .arg("-c")
            .arg(&cmd_str)
            .status()
            .map_err(|err| {
                CliError::LocalDNSInstall(format!(
                    "Failed to install resolver for domain {} to /etc/resolver/{}. Reason: {}",
                    domain, domain, err
                ))
            })?;

        if !status.success() {
            return Err(CliError::LocalDNSInstall(format!(
                "Failed to install resolver for domain {} to /etc/resolver/{}",
                domain, domain
            )));
        }
    }

    flush_dns_cache()?;

    #[cfg(target_os = "macos")]
    kill_dns_responder()?;

    Ok(())
}

fn uninstall_resolvers(resolve_domains: &[String]) -> Result<()> {
    for domain in resolve_domains.iter() {
        let folder = format!("/etc/resolver/{}", domain);
        Command::new("sudo")
            .args(["rm", "-rf", &folder])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| {
                CliError::LocalDNSUninstall(format!(
                    "Failed to delete /etc/resolver/{}. Reason: {}",
                    domain, err
                ))
            })?;
    }

    flush_dns_cache()?;

    #[cfg(target_os = "macos")]
    kill_dns_responder()?;

    Ok(())
}

pub fn list_resolvers() -> std::result::Result<Vec<String>, std::io::Error> {
    let resolvers_dir = match fs::read_dir("/etc/resolver/") {
        Ok(read_dir) => read_dir,
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound => return Ok(vec![]),
            _ => return Err(err),
        },
    };

    let resolvers = resolvers_dir
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    Ok(resolvers)
}

fn flush_dns_cache() -> Result<()> {
    #[cfg(target_os = "linux")]
    let status_flush = Command::new("resolvectl")
        .args(["flush-caches"])
        .status()
        .map_err(|_err| {
            CliError::LocalDNSInstall("Failed to run resolvectl flush-caches".into())
        })?;

    #[cfg(target_os = "macos")]
    let status_flush = Command::new("dscacheutil")
        .args(["-flushcache"])
        .status()
        .map_err(|_err| {
            CliError::LocalDNSInstall("Failed to run dscacheutil -flushcache".into())
        })?;

    if !status_flush.success() {
        return Err(CliError::LocalDNSInstall("Failed flush DNS cache".into()));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn kill_dns_responder() -> Result<()> {
    let status_kill_responder = Command::new("sudo")
        .args(["killall", "-HUP", "mDNSResponder"])
        .status()
        .map_err(|_err| {
            CliError::LocalDNSInstall("Failed to run killall -HUP mDNSResponder".into())
        })?;

    if !status_kill_responder.success() {
        return Err(CliError::LocalDNSInstall(
            "Failed to run killall -HUP mDNSResponder".into(),
        ));
    }

    Ok(())
}
