use std::path::PathBuf;

use crate::{
    Result,
    state::{config_path, get_config},
};
use linkup::MemoryStringStore;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    certs_dir: String,
}

pub async fn server(args: &Args, config_arg: &Option<String>) -> Result<()> {
    let config = get_config(&config_path(config_arg)?)?;

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
