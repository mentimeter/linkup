use crate::Result;
use linkup::MemoryStringStore;
use tokio::select;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    server_kind: ServerKind,
}

#[derive(clap::Subcommand)]
pub enum ServerKind {
    LocalWorker {
        #[arg(long)]
        certs_dir: String,
        #[arg(long, default_value_t = 80)]
        http_port: u16,
        #[arg(long, default_value_t = 443)]
        https_port: u16,
    },

    Dns {
        #[arg(long)]
        session_name: String,
        #[arg(long, value_parser, num_args = 1.., value_delimiter = ',')]
        domains: Vec<String>,
    },
}

pub async fn server(args: &Args) -> Result<()> {
    match &args.server_kind {
        ServerKind::LocalWorker {
            certs_dir,
            http_port,
            https_port,
        } => {
            let config_store = MemoryStringStore::default();

            let http_config_store = config_store.clone();
            let http_port = *http_port;
            let handler_http = tokio::spawn(async move {
                linkup_local_server::start_server_http(http_config_store, http_port)
                    .await
                    .unwrap();
            });

            let handler_https = {
                use std::path::PathBuf;

                let https_config_store = config_store.clone();
                let https_certs_dir = PathBuf::from(certs_dir);
                let https_port = *https_port;

                Some(tokio::spawn(async move {
                    linkup_local_server::start_server_https(
                        https_config_store,
                        &https_certs_dir,
                        https_port,
                    )
                    .await;
                }))
            };

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
