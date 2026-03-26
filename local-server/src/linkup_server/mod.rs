use std::{net::SocketAddr, path::Path, sync::Arc};

use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use http::StatusCode;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use linkup::MemoryStringStore;
use rustls::ServerConfig;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};

use crate::{certificates, dns_server::DnsCatalog, HttpsClient};

mod handlers;

#[derive(Debug)]
struct ApiError {
    message: String,
    status_code: StatusCode,
}

impl ApiError {
    fn new(message: String, status_code: StatusCode) -> Self {
        ApiError {
            message,
            status_code,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status_code)
            .header("Content-Type", "text/plain")
            .body(Body::from(self.message))
            .unwrap()
    }
}

pub async fn serve_http(config_store: MemoryStringStore, dns_catalog: DnsCatalog) {
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

pub async fn serve_https(
    config_store: MemoryStringStore,
    certs_dir: &Path,
    dns_catalog: DnsCatalog,
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

pub fn router(config_store: MemoryStringStore, dns_catalog: DnsCatalog) -> Router {
    let client = https_client();

    Router::new()
        .route(
            "/linkup/local-session",
            post(handlers::local_session::handle_upsert),
        )
        .route("/linkup/check", get(async || "Ok"))
        .route("/linkup/dns/records", post(handlers::dns::handle_create))
        .fallback(any(handlers::proxy::handle))
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
