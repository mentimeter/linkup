use std::path::{Path, PathBuf};

use linkup::MemoryStringStore;

use crate::{Result, config::load_config_with_override};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    certs_dir: String,
}

pub async fn server(args: &Args, config_arg: Option<&Path>) -> Result<()> {
    let (config, _) = load_config_with_override(config_arg)?;

    let config_store = MemoryStringStore::default();
    let https_certs_dir = PathBuf::from(&args.certs_dir);

    linkup_local_server::start(
        config_store,
        &https_certs_dir,
        &config.linkup.worker_url,
        &config.linkup.worker_token,
    )
    .await;

    Ok(())
}
