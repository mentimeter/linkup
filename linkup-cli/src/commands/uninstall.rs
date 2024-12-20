use std::fs;

use crate::{commands, linkup_dir_path, CliError};

#[derive(clap::Args)]
pub struct Args {}

pub fn uninstall(_args: &Args) -> Result<(), CliError> {
    commands::stop(&commands::StopArgs {}, true)?;
    fs::remove_dir_all(linkup_dir_path())?;

    println!("linkup uninstalled!");

    Ok(())
}
