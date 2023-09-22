use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    linkup_file_path,
    local_config::{config_path, get_config},
    CliError, Result, LINKUP_LOCALDNS_INSTALL,
};

pub fn install(config_arg: &Option<String>) -> Result<()> {
    let config_path = config_path(config_arg)?;
    let input_config = get_config(&config_path)?;

    println!("Installing local-dns requires sudo.");
    println!("Linkup will put files into /etc/resolver/<domain>.");
    ensure_resolver_dir()?;
    install_resolvers(&input_config.top_level_domains())?;

    if fs::write(linkup_file_path(LINKUP_LOCALDNS_INSTALL), "").is_err() {
        return Err(CliError::LocalDNSInstall(format!(
            "Failed to write install localdns file at {}",
            linkup_file_path(LINKUP_LOCALDNS_INSTALL).display()
        )));
    };

    Ok(())
}

pub fn uninstall(config: &Option<String>) -> Result<()> {
    todo!()
}

// ---

fn ensure_resolver_dir() -> Result<()> {
    Command::new("sudo")
        .args(&["mkdir", "/etc/resolver"])
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

fn install_resolvers(resolve_domains: &Vec<String>) -> Result<()> {
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

    let status_flush = Command::new("sudo")
        .args(&["dscacheutil", "-flushcache"])
        .status()
        .map_err(|err| CliError::LocalDNSInstall("Failed to run dscacheutil -flushcache".into()))?;

    if !status_flush.success() {
        return Err(CliError::LocalDNSInstall(
            "Failed to run dscacheutil -flushcache".into(),
        ));
    }

    let status_killall = Command::new("sudo")
        .args(&["killall", "-HUP", "mDNSResponder"])
        .status()
        .map_err(|err| {
            CliError::LocalDNSInstall("Failed to run killall -HUP mDNSResponder".into())
        })?;

    if !status_killall.success() {
        return Err(CliError::LocalDNSInstall(
            "Failed to run killall -HUP mDNSResponder".into(),
        ));
    }

    Ok(())
}
