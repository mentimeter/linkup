use std::{fs, process};

use crate::{
    commands::{self, local_dns},
    linkup_dir_path, linkup_exe_path, CliError, InstallationMethod,
};

#[derive(clap::Args)]
pub struct Args {}

pub async fn uninstall(_args: &Args, config_arg: &Option<String>) -> Result<(), CliError> {
    commands::stop(&commands::StopArgs {}, true)?;

    local_dns::uninstall(config_arg).await?;

    let exe_path = linkup_exe_path();

    log::debug!("Linkup exe path: {:?}", &exe_path);
    match InstallationMethod::current() {
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

            fs::remove_file(&exe_path)?;
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
