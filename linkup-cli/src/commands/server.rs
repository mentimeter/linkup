use std::path::PathBuf;

use crate::Result;
use linkup::MemoryStringStore;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    session_name: String,

    #[arg(long, value_parser, num_args = 1.., value_delimiter = ',')]
    domains: Vec<String>,

    #[arg(long)]
    certs_dir: String,
}

pub async fn server(args: &Args) -> Result<()> {
    let config_store = MemoryStringStore::default();
    let https_certs_dir = PathBuf::from(&args.certs_dir);

    linkup_local_server::start(
        config_store,
        &https_certs_dir,
        args.session_name.clone(),
        args.domains.clone(),
    )
    .await;

    Ok(())
}
