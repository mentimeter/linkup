pub mod certificates;
pub mod dns;
mod handlers;
mod ws;

use axum::{
    Router,
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
use linkup::{MemoryStringStore, SessionAllocator};
use linkup_clients::WorkerClient;
use rustls::ServerConfig;
use std::{net::SocketAddr, path::PathBuf};
use std::{path::Path, sync::Arc};
use tokio::{net::UdpSocket, select, signal};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use url::Url;

use crate::dns::DnsCatalog;

pub use linkup_clients::{HttpsClient, https_client};

type AxumHttpsClient = HttpsClient<axum::body::Body>;

#[derive(Clone)]
pub struct ServerState {
    pub dns_catalog: DnsCatalog,
    pub https_certs_dir: PathBuf,
    pub https_client: AxumHttpsClient,
    pub session_allocator: SessionAllocator<MemoryStringStore>,
    pub worker_client: WorkerClient,
}

pub fn router(server_state: ServerState) -> Router {
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
        .with_state(server_state)
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

pub async fn start(
    string_store: MemoryStringStore,
    certs_dir: &Path,
    worker_url: &Url,
    worker_token: &str,
) {
    let worker_client = WorkerClient::new(worker_url, worker_token);

    let server_state = ServerState {
        session_allocator: SessionAllocator::new(string_store),
        https_client: https_client(),
        dns_catalog: dns::DnsCatalog::new(),
        https_certs_dir: PathBuf::from(certs_dir),
        worker_client,
    };

    select! {
        () = start_server_http(server_state.clone()) => {
            println!("HTTP server shut down");
        },
        () = start_server_https(server_state.clone()) => {
            println!("HTTPS server shut down");
        },
        () = start_dns_server(server_state.dns_catalog) => {
            println!("DNS server shut down");
        },
        () = shutdown_signal() => {
            println!("Shutdown signal received, stopping all servers");
        }
    }
}

async fn start_server_https(server_state: ServerState) {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let sni = match certificates::WildcardSniResolver::load_dir(&server_state.https_certs_dir) {
        Ok(sni) => sni,
        Err(error) => {
            eprintln!(
                "Failed to load certificates from {:?} into SNI: {}",
                &server_state.https_certs_dir, error
            );
            return;
        }
    };

    let mut server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(sni));
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let app = router(server_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 443));
    println!("HTTPS listening on {}", &addr);

    axum_server::bind_rustls(addr, RustlsConfig::from_config(Arc::new(server_config)))
        .serve(app.into_make_service())
        .await
        .expect("failed to start HTTPS server");
}

async fn start_server_http(server_state: ServerState) {
    let app = router(server_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    println!("HTTP listening on {}", &addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind to address");

    axum::serve(listener, app)
        .await
        .expect("failed to start HTTP server");
}

async fn start_dns_server(dns_catalog: DnsCatalog) {
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
