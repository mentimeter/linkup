use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    commands, is_sudo, linkup_certs_dir_path,
    local_config::{self, managed_domains, top_level_domains, LocalState},
    sudo_su, Result,
};
use anyhow::{anyhow, Context};
use clap::Subcommand;
use linkup_local_server::certificates::{
    setup_self_signed_certificates, uninstall_self_signed_certificates,
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

pub async fn local_dns(args: &Args, config: &Option<String>) -> Result<()> {
    match args.subcommand {
        LocalDNSSubcommand::Install => install(config).await,
        LocalDNSSubcommand::Uninstall => uninstall(config).await,
    }
}

pub async fn install(config_arg: &Option<String>) -> Result<()> {
    // NOTE(augustoccesar)[2025-03-24] We decided to print this anyways, even if the current session already have sudo.
    // This should help with visibility of what is happening.
    println!("Linkup needs sudo access to:");
    println!("  - Ensure there is a folder /etc/resolvers");
    println!("  - Create file(s) for /etc/resolver/<domain>");
    println!("  - Add Linkup CA certificate to keychain");
    println!("  - Flush DNS cache");

    if !is_sudo() {
        sudo_su()?;
    }

    commands::stop(&commands::StopArgs {}, false)?;

    ensure_resolver_dir()?;

    let domains = managed_domains(LocalState::load().ok().as_ref(), config_arg);

    install_resolvers(&top_level_domains(&domains))?;

    setup_self_signed_certificates(&linkup_certs_dir_path(), &domains)
        .context("Failed to setup self-signed certificates")?;

    println!("Local DNS installed!");

    Ok(())
}

pub async fn uninstall(config_arg: &Option<String>) -> Result<()> {
    // NOTE(augustoccesar)[2025-03-24] We decided to print this anyways, even if the current session already have sudo.
    // This should help with visibility of what is happening.
    println!("Linkup needs sudo access to:");
    println!("  - Delete file(s) on /etc/resolver");
    println!("  - Remove Linkup CA certificate from keychain");
    println!("  - Flush DNS cache");

    if !is_sudo() {
        sudo_su()?;
    }

    commands::stop(&commands::StopArgs {}, false)?;

    let managed_top_level_domains = local_config::top_level_domains(
        &local_config::managed_domains(LocalState::load().ok().as_ref(), config_arg),
    );

    uninstall_resolvers(&managed_top_level_domains)?;
    uninstall_self_signed_certificates(&linkup_certs_dir_path())
        .context("Failed to uninstall self-signed certificates")?;

    println!("Local DNS uninstalled!");

    Ok(())
}

fn ensure_resolver_dir() -> Result<()> {
    Command::new("sudo")
        .args(["mkdir", "/etc/resolver"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to create /etc/resolver folder")?;

    Ok(())
}

pub fn is_installed(managed_domains: &[String]) -> bool {
    match list_resolvers() {
        Ok(resolvers) => managed_domains
            .iter()
            .any(|domain| resolvers.contains(domain)),
        Err(error) => {
            log::error!("Failed to load resolvers: {}", error);

            false
        }
    }
}

fn install_resolvers(resolve_domains: &[String]) -> Result<()> {
    for domain in resolve_domains.iter() {
        let cmd_str = format!("echo \"nameserver 127.0.0.1\nport 8053\" > /etc/resolver/{domain}");

        let status = Command::new("sudo")
            .arg("bash")
            .arg("-c")
            .arg(&cmd_str)
            .status()
            .with_context(|| {
                format!("Failed to install resolver for domain {domain} to /etc/resolver/{domain}")
            })?;

        if !status.success() {
            return Err(anyhow!(
                "Failed to install resolver for domain {domain} to /etc/resolver/{domain}"
            ));
        }
    }

    flush_dns_cache()?;
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
            .with_context(|| format!("Failed to delete /etc/resolver/{domain}",))?;
    }

    #[cfg(target_os = "macos")]
    {
        flush_dns_cache()?;
        kill_dns_responder()?;
    }

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

#[cfg(target_os = "macos")]
fn flush_dns_cache() -> Result<()> {
    let status_flush = Command::new("dscacheutil")
        .args(["-flushcache"])
        .status()
        .context("Failed to flush DNS cache")?;

    if !status_flush.success() {
        return Err(anyhow!("Flushing DNS cache was unsuccessful"));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn flush_dns_cache() -> Result<()> {
    let status_flush = Command::new("sudo")
        .args(["resolvectl", "flush-caches"])
        .status()
        .context("Failed to flush DNS cache")?;

    if !status_flush.success() {
        log::warn!("Flushing DNS cache was unsuccessful");
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn kill_dns_responder() -> Result<()> {
    let status_kill_responder = Command::new("sudo")
        .args(["killall", "-HUP", "mDNSResponder"])
        .status()
        .context("Failed to kill DNS responder")?;

    if !status_kill_responder.success() {
        return Err(anyhow!("Killing DNS responder was unsuccessful"));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn kill_dns_responder() -> Result<()> {
    let status_kill_responder = Command::new("sudo")
        .args(["killall", "-USR2", "systemd-resolved"])
        .status()
        .context("Failed to kill DNS responder")?;

    if !status_kill_responder.success() {
        log::warn!("Killing DNS responder was unsuccessful");
    }

    Ok(())
}
