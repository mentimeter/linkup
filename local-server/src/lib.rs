pub mod certificates;
pub mod dns;
mod handlers;
mod ws;

use axum::{
    Extension, Router,
    body::Body,
    extract::DefaultBodyLimit,
    routing::{any, get, post},
};
use axum_server::tls_rustls::RustlsConfig;
use hickory_server::net::runtime::TokioRuntimeProvider;
use hickory_server::store::forwarder::ForwardZoneHandler;
use hickory_server::{
    proto::rr::Name,
    resolver::config::{NameServerConfig, ResolverOpts},
    store::forwarder::ForwardConfig,
};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use linkup::MemoryStringStore;
use rustls::ServerConfig;
use std::{net::SocketAddr, path::PathBuf};
use std::{path::Path, sync::Arc};
use tokio::{net::UdpSocket, select, signal};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};

type HttpsClient = Client<HttpsConnector<HttpConnector>, Body>;

pub fn router(config_store: MemoryStringStore, dns_catalog: dns::DnsCatalog) -> Router {
    let client = https_client();

    Router::new()
        .route(
            "/linkup/sessions/preview",
            post(handlers::sessions::upsert_preview),
        )
        .route(
            "/linkup/sessions/tunneled",
            post(handlers::sessions::upsert_tunneled),
        )
        .route(
            "/linkup/sessions/local-only",
            post(handlers::sessions::upsert_local_only),
        )
        .route("/linkup/check", get(handlers::always_ok))
        .fallback(any(handlers::proxy::handle_all))
        .layer(Extension(config_store))
        .layer(Extension(dns_catalog))
        .layer(Extension(client))
        .layer(
            ServiceBuilder::new()
                .layer(DefaultBodyLimit::max(1024 * 1024 * 100)) // Set max body size to 100MB
                .layer(
                    TraceLayer::new_for_http()
                        .on_request(DefaultOnRequest::new()) // Log all incoming requests at INFO level
                        .on_response(DefaultOnResponse::new()), // Log all responses at INFO level
                ),
        )
}

pub async fn start(config_store: MemoryStringStore, certs_dir: &Path) {
    let dns_catalog = dns::DnsCatalog::new();

    let http_config_store = config_store.clone();
    let https_config_store = config_store.clone();
    let https_certs_dir = PathBuf::from(certs_dir);

    select! {
        () = start_server_http(http_config_store, dns_catalog.clone()) => {
            println!("HTTP server shut down");
        },
        () = start_server_https(https_config_store, &https_certs_dir, dns_catalog.clone()) => {
            println!("HTTPS server shut down");
        },
        () = start_dns_server(dns_catalog.clone()) => {
            println!("DNS server shut down");
        },
        () = shutdown_signal() => {
            println!("Shutdown signal received, stopping all servers");
        }
    }
}

async fn start_server_https(
    config_store: MemoryStringStore,
    certs_dir: &Path,
    dns_catalog: dns::DnsCatalog,
) {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let sni = match certificates::WildcardSniResolver::load_dir(certs_dir) {
        Ok(sni) => sni,
        Err(error) => {
            eprintln!(
                "Failed to load certificates from {:?} into SNI: {}",
                certs_dir, error
            );
            return;
        }
    };

    let mut server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(sni));
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let app = router(config_store, dns_catalog);

    let addr = SocketAddr::from(([0, 0, 0, 0], 443));
    println!("HTTPS listening on {}", &addr);

    axum_server::bind_rustls(addr, RustlsConfig::from_config(Arc::new(server_config)))
        .serve(app.into_make_service())
        .await
        .expect("failed to start HTTPS server");
}

async fn start_server_http(config_store: MemoryStringStore, dns_catalog: dns::DnsCatalog) {
    let app = router(config_store, dns_catalog);

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    println!("HTTP listening on {}", &addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind to address");

    axum::serve(listener, app)
        .await
        .expect("failed to start HTTP server");
}

async fn start_dns_server(dns_catalog: dns::DnsCatalog) {
    let cf_name_server = NameServerConfig::udp("1.1.1.1".parse().unwrap());
    let forward_config = ForwardConfig {
        name_servers: vec![cf_name_server],
        options: Some(ResolverOpts::default()),
    };

    let forwarder =
        ForwardZoneHandler::builder_with_config(forward_config, TokioRuntimeProvider::default())
            .with_origin(Name::root())
            .build()
            .unwrap();

    {
        let mut catalog = dns_catalog.write().await;
        catalog.upsert(Name::root().into(), vec![Arc::new(forwarder)]);
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], 8053));
    let sock = UdpSocket::bind(&addr).await.unwrap();

    let mut server = hickory_server::Server::new(dns_catalog);
    server.register_socket(sock);

    println!("listening on {addr}");
    server.block_until_done().await.unwrap();
}

fn https_client() -> HttpsClient {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().expect("could not load platform certs") {
        roots.add(cert).unwrap();
    }

    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    Client::builder(TokioExecutor::new()).build(https)
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
