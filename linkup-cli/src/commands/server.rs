use crate::Result;
use linkup::MemoryStringStore;
use std::fs;
use tokio::select;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pidfile: String,

    #[command(subcommand)]
    server_kind: ServerKind,
}

#[derive(clap::Subcommand)]
pub enum ServerKind {
    LocalWorker {
        #[arg(long)]
        certs_dir: String,
    },

    Dns {
        #[arg(long)]
        session_name: String,
        #[arg(long, value_parser, num_args = 1.., value_delimiter = ',')]
        domains: Vec<String>,
    },
}

pub async fn server(args: &Args) -> Result<()> {
    let pid = std::process::id();
    fs::write(&args.pidfile, pid.to_string())?;

    match &args.server_kind {
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        ServerKind::LocalWorker { certs_dir } => {
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
                    linkup_local_server::start_server_https(https_config_store, &https_certs_dir)
                        .await;
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
        }
        ServerKind::Dns {
            session_name,
            domains,
        } => {
            let session_name = session_name.clone();
            let domains = domains.clone();

            let handler_dns = tokio::spawn(async move {
                linkup_local_server::start_dns_server(session_name, domains).await;
            });

            handler_dns.await.unwrap();
        }
    }

    Ok(())
}
