use crate::CliError;
use linkup::MemoryStringStore;
use std::fs;
use std::path::Path;
use tokio::select;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pidfile: String,
}

#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub async fn server(args: &Args, certs_dir: &Path) -> Result<(), CliError> {
    let pid = std::process::id();
    fs::write(&args.pidfile, pid.to_string())?;

    let config_store = MemoryStringStore::default();

    let http_config_store = config_store.clone();
    let handler_http = tokio::spawn(async move {
        linkup_local_server::start_server_http(http_config_store)
            .await
            .unwrap();
    });

    #[cfg(target_os = "macos")]
    let handler_https = {
        use std::path::PathBuf;

        let https_config_store = config_store.clone();
        let https_certs_dir = PathBuf::from(certs_dir);

        Some(tokio::spawn(async move {
            linkup_local_server::start_server_https(https_config_store, &https_certs_dir).await;
        }))
    };

    #[cfg(not(target_os = "macos"))]
    let handler_https: Option<tokio::task::JoinHandle<()>> = None;

    match handler_https {
        Some(handler_https) => {
            select! {
                _ = handler_http => (),
                _ = handler_https => (),
            }
        }
        None => {
            handler_http.await.unwrap();
        }
    }

    Ok(())
}
