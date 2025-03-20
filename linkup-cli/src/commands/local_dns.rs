use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    commands, is_sudo, linkup_certs_dir_path,
    local_config::{config_path, get_config, LocalState},
    sudo_su, CliError, Result,
};
use clap::Subcommand;
use linkup_local_server::certificates::setup_self_signed_certificates;

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
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Ensure there is a folder /etc/resolvers");
        println!("  - Create file(s) for /etc/resolver/<domain>");
        println!("  - Add Linkup CA certificate to keychain");
        println!("  - Register port forwarding for 80 and 443");
        println!("  - Flush DNS cache");

        sudo_su()?;
    }

    commands::stop(&commands::StopArgs {}, false)?;

    ensure_resolver_dir()?;
    install_resolvers(&input_config.top_level_domains())?;

    let domains = input_config
        .domains
        .iter()
        .map(|storable_domain| storable_domain.domain.clone())
        .collect::<Vec<String>>();

    setup_self_signed_certificates(&linkup_certs_dir_path(), &domains).map_err(|error| {
        CliError::LocalDNSInstall(format!(
            "Failed to setup self signed certificates: {}",
            error
        ))
    })?;

    linkup_local_server::setup_port_forwarding()
        .map_err(|error| CliError::SetupPortForwarding(error.to_string()))?;

    Ok(())
}

// TODO(augustoccesar)[2025-03-20]: Remove Linkup CA from keychain on uninstall
pub async fn uninstall(config_arg: &Option<String>) -> Result<()> {
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Delete file(s) on /etc/resolver");
        println!("  - Reset port forwarding rules");
        println!("  - Flush DNS cache");
    }

    commands::stop(&commands::StopArgs {}, false)?;

    uninstall_resolvers(&input_config.top_level_domains())?;
    linkup_local_server::reset_port_forwarding().map_err(|error| {
        CliError::LocalDNSUninstall(format!("Failed to reset port forwarding: {error}"))
    })?;

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

pub fn is_installed(state: Option<&LocalState>) -> bool {
    match state {
        Some(state) => match list_resolvers() {
            Ok(resolvers) => state
                .domain_strings()
                .iter()
                .any(|domain| resolvers.contains(domain)),
            Err(error) => {
                log::error!("Failed to load resolvers: {}", error);

                false
            }
        },
        None => false,
    }
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
