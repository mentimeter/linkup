use std::{fs, process};

use crate::{
    commands, commands::local_dns, linkup_dir_path, linkup_exe_path, local_config::managed_domains,
    local_config::LocalState, prompt, InstallationMethod, Result,
};

#[cfg(target_os = "linux")]
use crate::{is_sudo, sudo_su};

#[derive(clap::Args)]
pub struct Args {}

pub async fn uninstall(_args: &Args, config_arg: &Option<String>) -> Result<()> {
    let response = prompt("Are you sure you want to uninstall linkup? [y/N]: ")
        .trim()
        .to_lowercase();

    if !matches!(response.as_str(), "y" | "yes") {
        println!("Aborted!");

        return Ok(());
    }

    commands::stop(&commands::StopArgs {}, true)?;

    if local_dns::is_installed(&managed_domains(
        LocalState::load().ok().as_ref(),
        config_arg,
    )) {
        local_dns::uninstall(config_arg).await?;
    }

    let exe_path = linkup_exe_path()?;

    log::debug!("Linkup exe path: {:?}", &exe_path);
    match InstallationMethod::current()? {
        InstallationMethod::Brew => {
            log::debug!("Uninstalling linkup from Homebrew");

            process::Command::new("brew")
                .args(["uninstall", "linkup"])
                .stdin(process::Stdio::null())
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .status()?;
        }
        InstallationMethod::Cargo => {
            log::debug!("Uninstalling linkup from Cargo");

            process::Command::new("cargo")
                .args(["uninstall", "linkup-cli"])
                .stdin(process::Stdio::null())
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .status()?;
        }
        InstallationMethod::Manual => {
            log::debug!("Uninstalling linkup");

            #[cfg(target_os = "linux")]
            {
                println!("Linkup needs sudo access to:");
                println!("  - Remove binary from {:?}", &exe_path);

                if !is_sudo() {
                    sudo_su()?;
                }

                process::Command::new("sudo")
                    .args(["rm", "-f"])
                    .arg(&exe_path)
                    .stdin(process::Stdio::null())
                    .stdout(process::Stdio::null())
                    .stderr(process::Stdio::null())
                    .status()?;
            }

            #[cfg(not(target_os = "linux"))]
            {
                fs::remove_file(&exe_path)?;
            }
        }
    }

    let linkup_dir = linkup_dir_path();

    log::debug!("Removing linkup folder: {}", linkup_dir.display());
    if let Err(error) = fs::remove_dir_all(linkup_dir) {
        match error.kind() {
            std::io::ErrorKind::NotFound => (),
            _ => return Err(error.into()),
        }
    }

    println!("linkup uninstalled!");

    Ok(())
}
