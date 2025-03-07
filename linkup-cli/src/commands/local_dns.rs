use std::{
    fs,
    process::{Command, Stdio},
};

use clap::Subcommand;

use crate::{
    commands, is_sudo, linkup_certs_dir_path,
    local_config::{config_path, get_config},
    sudo_su, CliError, Result,
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
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Ensure there is a folder /etc/resolvers");
        println!("  - Create file(s) for /etc/resolver/<domain>");
        println!("  - Flush DNS cache");

        sudo_su()?;
    }

    commands::stop(&commands::StopArgs {}, false)?;

    ensure_resolver_dir()?;
    install_resolvers(&input_config.top_level_domains())?;

    ensure_certs_dir()?;
    let certs_dir = linkup_certs_dir_path();
    linkup_local_server::certificates::upsert_ca_cert(&certs_dir);
    linkup_local_server::certificates::add_ca_to_keychain(&certs_dir);
    linkup_local_server::certificates::install_nss();
    linkup_local_server::certificates::add_ca_to_nss(&certs_dir);

    for domain in input_config
        .domains
        .iter()
        .map(|storable_domain| storable_domain.domain.clone())
        .collect::<Vec<String>>()
    {
        linkup_local_server::certificates::create_domain_cert(&certs_dir, &format!("*.{}", domain));
    }

    Ok(())
}

pub async fn uninstall(config_arg: &Option<String>) -> Result<()> {
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    if !is_sudo() {
        println!("Linkup needs sudo access to:");
        println!("  - Delete file(s) on /etc/resolver");
        println!("  - Flush DNS cache");
    }

    commands::stop(&commands::StopArgs {}, false)?;

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

fn ensure_certs_dir() -> Result<()> {
    let path = linkup_certs_dir_path();
    if !path.exists() {
        fs::create_dir_all(path)?;
    }

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
