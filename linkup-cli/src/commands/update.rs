use crate::{current_version, release, CliError};
use std::{fs, path::PathBuf};

#[derive(clap::Args)]
pub struct Args {}

pub async fn update(_args: &Args) -> Result<(), CliError> {
    match release::available_update(&current_version()).await {
        Some(update) => {
            let new_linkup_path = update.linkup.download_decompressed("linkup").await.unwrap();

            let current_linkup_path = get_exe_path().expect("failed to get the current exe path");
            let bkp_linkup_path = current_linkup_path.with_extension("bkp");

            fs::rename(&current_linkup_path, &bkp_linkup_path)
                .expect("failed to move the current exe into a backup");
            fs::rename(&new_linkup_path, &current_linkup_path)
                .expect("failed to move the new exe as the current exe");

            let new_caddy_path = update.caddy.download_decompressed("caddy").await.unwrap();

            let current_caddy_path = get_caddy_path();
            let bkp_caddy_path = current_caddy_path.with_extension("bkp");

            fs::rename(&current_caddy_path, &bkp_caddy_path)
                .expect("failed to move the current exe into a backup");
            fs::rename(&new_caddy_path, &current_caddy_path)
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

fn get_caddy_path() -> PathBuf {
    let mut path = crate::linkup_bin_dir_path();
    path.push("caddy");

    path
}
