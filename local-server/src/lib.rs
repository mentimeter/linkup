pub mod certificates;
mod port_forwarding;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Json, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use http::{header::HeaderMap, Uri};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
};
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, MemoryStringStore, NameKind,
    Session, SessionAllocator, TargetService, UpdateSessionRequest,
};
use rustls::ServerConfig;
use std::net::{SocketAddr, TcpListener};
use std::{path::Path, sync::Arc};
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};

pub use port_forwarding::{
    is_port_forwarding_active, reset_port_forwarding, setup_port_forwarding,
};

pub const HTTP_PORT: u16 = 8080;
pub const HTTPS_PORT: u16 = 8443;

type HttpsClient = Client<HttpsConnector<HttpConnector>, Body>;

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

pub fn linkup_router(config_store: MemoryStringStore) -> Router {
    let client = https_client();

    Router::new()
        .route("/linkup/local-session", post(linkup_config_handler))
        .route("/linkup/check", get(always_ok))
        .fallback(any(linkup_request_handler))
        .layer(Extension(config_store))
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

pub async fn start_server_https(config_store: MemoryStringStore, certs_dir: &Path) {
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

    let app = linkup_router(config_store);

    let addr = SocketAddr::from(([127, 0, 0, 1], HTTPS_PORT));
    println!("listening on {}", &addr);

    axum_server::bind_rustls(addr, RustlsConfig::from_config(Arc::new(server_config)))
        .serve(app.into_make_service())
        .await
        .expect("failed to start HTTPS server");
}

pub async fn start_server_http(config_store: MemoryStringStore) -> std::io::Result<()> {
    let app = linkup_router(config_store);

    let addr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    println!("listening on {}", &addr);

    let listener = TcpListener::bind(addr)?;

    axum_server::Server::from_tcp(listener)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

#[tokio::main]
pub async fn local_linkup_main() -> std::io::Result<()> {
    start_server_http(MemoryStringStore::default()).await
}

async fn linkup_request_handler(
    Extension(store): Extension<MemoryStringStore>,
    Extension(client): Extension<HttpsClient>,
    req: Request,
) -> Response {
    let sessions = SessionAllocator::new(&store);

    let headers: linkup::HeaderMap = req.headers().into();
    let url = if req.uri().scheme().is_some() {
        req.uri().to_string()
    } else {
        format!(
            "http://{}{}",
            req.headers()
                .get(http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or(&format!("localhost:{}", HTTP_PORT)),
            req.uri()
        )
    };

    let (session_name, config) = match sessions.get_request_session(&url, &headers).await {
        Ok(session) => session,
        Err(_) => {
            return ApiError::new(
                "Linkup was unable to determine the session origin of the request. Ensure that your request includes a valid session identifier in the referer or tracestate headers. - Local Server".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
                .into_response()
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return ApiError::new(
                "The request belonged to a session, but there was no target for the request. Check that the routing rules in your linkup config have a match for this request. - Local Server".to_string(),
                StatusCode::NOT_FOUND,
            )
                .into_response()
        }
    };

    let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);

    if req
        .headers()
        .get("upgrade")
        .map(|v| v == "websocket")
        .unwrap_or(false)
    {
        handle_ws_req(req, target_service, extra_headers, client).await
    } else {
        handle_http_req(req, target_service, extra_headers, client).await
    }
}

async fn handle_http_req(
    mut req: Request,
    target_service: TargetService,
    extra_headers: linkup::HeaderMap,
    client: HttpsClient,
) -> Response {
    *req.uri_mut() = Uri::try_from(&target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    // Request uri and host headers should not conflict
    req.headers_mut().remove(http::header::HOST);

    if target_service.url.starts_with("http://") {
        *req.version_mut() = http::Version::HTTP_11;
    }

    // Send the modified request to the target service.
    let mut resp = match client.request(req).await {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!(
                    "Failed to proxy request - are all your servers started? {}",
                    e
                ),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    resp.headers_mut().extend(allow_all_cors());

    resp.into_response()
}

async fn handle_ws_req(
    req: Request,
    target_service: TargetService,
    extra_headers: linkup::HeaderMap,
    client: HttpsClient,
) -> Response {
    let extra_http_headers: HeaderMap = extra_headers.into();

    let target_ws_req_result = Request::builder()
        .uri(target_service.url)
        .method(req.method().clone())
        .body(Body::empty());

    let mut target_ws_req = match target_ws_req_result {
        Ok(request) => request,
        Err(e) => {
            return ApiError::new(
                format!("Failed to build request: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    target_ws_req.headers_mut().extend(req.headers().clone());
    target_ws_req.headers_mut().extend(extra_http_headers);
    target_ws_req.headers_mut().remove(http::header::HOST);

    // Send the modified request to the target service.
    let target_ws_resp = match client.request(target_ws_req).await {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!("Failed to proxy request: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let status = target_ws_resp.status();
    if status != 101 {
        return ApiError::new(
            format!(
                "Failed to proxy request: expected 101 Switching Protocols, got {}",
                status
            ),
            StatusCode::BAD_GATEWAY,
        )
        .into_response();
    }

    let target_ws_resp_headers = target_ws_resp.headers().clone();

    let upgraded_target = match hyper::upgrade::on(target_ws_resp).await {
        Ok(upgraded) => upgraded,
        Err(e) => {
            return ApiError::new(
                format!("Failed to upgrade connection: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    tokio::spawn(async move {
        // We won't get passed this until the 101 response returns to the client
        let upgraded_incoming = match hyper::upgrade::on(req).await {
            Ok(upgraded) => upgraded,
            Err(e) => {
                println!("Failed to upgrade incoming connection: {}", e);
                return;
            }
        };

        let mut incoming_stream = TokioIo::new(upgraded_incoming);
        let mut target_stream = TokioIo::new(upgraded_target);

        let res = tokio::io::copy_bidirectional(&mut incoming_stream, &mut target_stream).await;

        match res {
            Ok((incoming_to_target, target_to_incoming)) => {
                println!(
                    "Copied {} bytes from incoming to target and {} bytes from target to incoming",
                    incoming_to_target, target_to_incoming
                );
            }
            Err(e) => {
                eprintln!("Error copying between incoming and target: {}", e);
            }
        }
    });

    let mut resp_builder = Response::builder().status(101);
    let resp_headers_result = resp_builder.headers_mut();
    if let Some(resp_headers) = resp_headers_result {
        for (header, value) in target_ws_resp_headers {
            if let Some(header_name) = header {
                resp_headers.append(header_name, value);
            }
        }
    }

    match resp_builder.body(Body::empty()) {
        Ok(response) => response,
        Err(e) => ApiError::new(
            format!("Failed to build response: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    }
}

async fn linkup_config_handler(
    Extension(store): Extension<MemoryStringStore>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let desired_name = update_req.desired_name.clone();
    let server_conf: Session = match update_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let sessions = SessionAllocator::new(&store);
    let session_name = sessions
        .store_session(server_conf, NameKind::Animal, desired_name)
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return ApiError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };

    (StatusCode::OK, name).into_response()
}

async fn always_ok() -> &'static str {
    "OK"
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
