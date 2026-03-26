use std::path::PathBuf;

use crate::Result;
use linkup::MemoryStringStore;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    certs_dir: String,
}

pub async fn server(args: &Args) -> Result<()> {
    let config_store = MemoryStringStore::default();
    let https_certs_dir = PathBuf::from(&args.certs_dir);

    linkup_local_server::start(config_store, &https_certs_dir).await;

    Ok(())
}
