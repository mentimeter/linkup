use crate::{current_version, release, CliError};
use std::{fs, path::PathBuf};

#[derive(clap::Args)]
pub struct Args {}

pub async fn update(_args: &Args) -> Result<(), CliError> {
    match release::available_update(&current_version()).await {
        Some(asset) => {
            let new_exe_path = asset.download_decompressed("linkup").await.unwrap();

            let current_exe = get_exe_path().expect("failed to get the current exe path");
            let bkp_exe = current_exe.with_extension("bkp");

            fs::rename(&current_exe, &bkp_exe)
                .expect("failed to move the current exe into a backup");
            fs::rename(&new_exe_path, &current_exe)
                .expect("failed to move the new exe as the current exe");

            println!("Finished update!");
        }
        None => {
            println!("No new version available.");
        }
    }

    Ok(())
}

pub async fn new_version_available() -> bool {
    release::available_update(&current_version())
        .await
        .is_some()
}

// Get the current exe path. Using canonicalize ensure that we follow the symlink in case it is one.
// This is important in case the version is one installed with Homebrew.
fn get_exe_path() -> Result<PathBuf, std::io::Error> {
    fs::canonicalize(std::env::current_exe()?)
}
