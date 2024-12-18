use std::fs;

use crate::{
    linkup_dir_path,
    stop::{self},
    CliError,
};

pub fn uninstall() -> Result<(), CliError> {
    stop::stop(true)?;
    fs::remove_dir_all(linkup_dir_path())?;

    println!("linkup uninstalled!");

    Ok(())
}
