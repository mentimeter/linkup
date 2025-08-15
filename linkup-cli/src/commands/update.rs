use anyhow::Context;
use std::fs;

use crate::{commands, current_version, linkup_exe_path, release, InstallationMethod, Result};

#[derive(clap::Args)]
pub struct Args {
    /// Ignore the cached last version and check remote server again for the latest version.
    #[arg(long)]
    skip_cache: bool,

    /// Which channel to update to/with.
    #[arg(long)]
    channel: Option<DesiredChannel>,
}

#[derive(Clone, clap::ValueEnum)]
enum DesiredChannel {
    Stable,
    Beta,
}

impl From<&DesiredChannel> for linkup::VersionChannel {
    fn from(value: &DesiredChannel) -> Self {
        match value {
            DesiredChannel::Stable => linkup::VersionChannel::Stable,
            DesiredChannel::Beta => linkup::VersionChannel::Beta,
        }
    }
}

pub async fn update(args: &Args) -> Result<()> {
    let current_version = current_version();

    if args.skip_cache {
        log::debug!("Clearing cache to force a new check for the latest version.");

        release::CachedReleases::clear();
    }

    let requested_channel = args.channel.as_ref().map(linkup::VersionChannel::from);

    match release::check_for_update(&current_version, requested_channel).await {
        Some(update) => {
            commands::stop(&commands::StopArgs {}, false)?;

            println!(
                "Updating from version '{}' ({}) to '{}' ({})...",
                &current_version,
                &current_version.channel(),
                &update.version,
                &update.version.channel()
            );

            let new_linkup_path = update
                .binary
                .download()
                .await
                .with_context(|| "Failed to download new version")?;

            let current_linkup_path = linkup_exe_path()?;
            let bkp_linkup_path = current_linkup_path.with_extension("bkp");

            fs::rename(&current_linkup_path, &bkp_linkup_path)
                .expect("failed to move the current exe into a backup");
            fs::rename(&new_linkup_path, &current_linkup_path)
                .expect("failed to move the new exe as the current exe");

            #[cfg(target_os = "linux")]
            {
                println!("Linkup needs sudo access to:");
                println!("  - Add capability to bind to port 80/443");
                std::process::Command::new("sudo")
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .stdin(std::process::Stdio::null())
                    .args([
                        "setcap",
                        "cap_net_bind_service=+ep",
                        &current_linkup_path.display().to_string(),
                    ])
                    .spawn()?;
            }

            println!("Finished update!");
        }
        None => {
            println!("No new version available.");
        }
    }

    Ok(())
}

pub async fn new_version_available() -> bool {
    release::check_for_update(&current_version(), None)
        .await
        .is_some()
}

pub fn update_command() -> Result<String> {
    match InstallationMethod::current()? {
        InstallationMethod::Brew => Ok("brew upgrade linkup".to_string()),
        InstallationMethod::Manual | InstallationMethod::Cargo => Ok("linkup update".to_string()),
    }
}
