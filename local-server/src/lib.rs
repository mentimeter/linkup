use axum::body::Body;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::{connect::HttpConnector, Client};
use linkup::MemoryStringStore;
use std::path::{Path, PathBuf};
use tokio::{select, signal};

pub mod certificates;
mod dns_server;
mod linkup_server;
mod ws;

pub use dns_server::DnsCatalog;
pub use linkup_server::router;

type HttpsClient = Client<HttpsConnector<HttpConnector>, Body>;

pub async fn start(config_store: MemoryStringStore, certs_dir: &Path) {
    let dns_catalog = DnsCatalog::new();

    let http_config_store = config_store.clone();
    let https_config_store = config_store.clone();
    let https_certs_dir = PathBuf::from(certs_dir);

    select! {
        () = linkup_server::serve_http(http_config_store, dns_catalog.clone()) => {
            println!("HTTP server shut down");
        },
        () = linkup_server::serve_https(https_config_store, &https_certs_dir, dns_catalog.clone()) => {
            println!("HTTPS server shut down");
        },
        () = dns_server::serve(dns_catalog.clone()) => {
            println!("DNS server shut down");
        },
        () = shutdown_signal() => {
            println!("Shutdown signal received, stopping all servers");
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to start SIGINT handler");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to start SIGTERM handler")
            .recv()
            .await;
    };

    tokio::select! {
        () = ctrl_c => {
            println!("Received SIGINT signal");
        },
        () = terminate => {
            println!("Received SIGTERM signal");
        },
    }
}
