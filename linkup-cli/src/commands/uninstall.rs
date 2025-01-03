use std::{fs, process};

use crate::{commands, linkup_dir_path, CliError};

#[derive(clap::Args)]
pub struct Args {}

pub fn uninstall(_args: &Args) -> Result<(), CliError> {
    commands::stop(&commands::StopArgs {}, true)?;

    let linkup_dir = linkup_dir_path();

    log::debug!("Removing linkup folder: {}", linkup_dir.display());
    fs::remove_dir_all(linkup_dir)?;

    let exe_path = fs::canonicalize(std::env::current_exe()?)?
        .display()
        .to_string();

    log::debug!("Linkup exe path: {}", &exe_path);
    if exe_path.contains("homebrew") {
        log::debug!("Uninstalling linkup from Homebrew");

        process::Command::new("brew")
            .args(["uninstall", "linkup"])
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .status()?;
    } else if exe_path.contains(".cargo") {
        log::debug!("Uninstalling linkup from Cargo");

        process::Command::new("cargo")
            .args(["uninstall", "linkup-cli"])
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .status()?;
    }

    println!("linkup uninstalled!");

    Ok(())
}
