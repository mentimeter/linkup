use linkup::MemoryStringStore;
use std::fs;
use std::path::PathBuf;
use tokio::select;

use crate::CliError;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pidfile: String,
}

pub async fn server(args: &Args, certs_dir: &PathBuf) -> Result<(), CliError> {
    let pid = std::process::id();
    fs::write(&args.pidfile, pid.to_string())?;

    let config_store = MemoryStringStore::default();

    let http_config_store = config_store.clone();
    let handler_http = tokio::spawn(async move {
        linkup_local_server::start_server_http(http_config_store)
            .await
            .unwrap();
    });

    let https_config_store = config_store.clone();
    let https_certs_dir = certs_dir.clone();
    let handler_https = tokio::spawn(async move {
        linkup_local_server::start_server_https(https_config_store, &https_certs_dir)
            .await
            .unwrap();
    });

    select! {
        _ = handler_http => (),
        _ = handler_https => (),
    }

    Ok(())
}
