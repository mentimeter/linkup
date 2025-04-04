use crate::{current_version, linkup_exe_path, release, InstallationMethod, Result};
use std::fs;

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
    if args.skip_cache {
        log::debug!("Clearing cache to force a new check for the latest version.");

        release::clear_cache();
    }

    let requested_channel = args.channel.as_ref().map(linkup::VersionChannel::from);

    match release::available_update(&current_version(), requested_channel).await {
        Some(update) => {
            let new_linkup_path = update.linkup.download_decompressed("linkup").await.unwrap();

            let current_linkup_path = linkup_exe_path()?;
            let bkp_linkup_path = current_linkup_path.with_extension("bkp");

            fs::rename(&current_linkup_path, &bkp_linkup_path)
                .expect("failed to move the current exe into a backup");
            fs::rename(&new_linkup_path, &current_linkup_path)
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
    release::available_update(&current_version(), None)
        .await
        .is_some()
}

pub fn update_command() -> Result<String> {
    match InstallationMethod::current()? {
        InstallationMethod::Brew => Ok("brew upgrade linkup".to_string()),
        InstallationMethod::Manual | InstallationMethod::Cargo => Ok("linkup update".to_string()),
    }
}
