use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Json, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use hickory_server::{
    authority::{Catalog, ZoneType},
    proto::{
        rr::{Name, RData, Record},
        xfer::Protocol,
    },
    resolver::{
        config::{NameServerConfig, NameServerConfigGroup, ResolverOpts},
        name_server::TokioConnectionProvider,
    },
    store::{
        forwarder::{ForwardAuthority, ForwardConfig},
        in_memory::InMemoryAuthority,
    },
    ServerFuture,
};
use http::{header::HeaderMap, HeaderName, HeaderValue, Uri};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, MemoryStringStore, NameKind,
    Session, SessionAllocator, TargetService, UpdateSessionRequest,
};
use rustls::ServerConfig;
use std::{
    net::{Ipv4Addr, SocketAddr},
    str::FromStr,
};
use std::{path::Path, sync::Arc};
use tokio::{net::UdpSocket, signal};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};

pub mod certificates;
mod ws;

type HttpsClient = Client<HttpsConnector<HttpConnector>, Body>;

const DISALLOWED_HEADERS: [HeaderName; 2] = [
    HeaderName::from_static("content-encoding"),
    HeaderName::from_static("content-length"),
];

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

    let addr = SocketAddr::from(([0, 0, 0, 0], 443));
    println!("listening on {}", &addr);

    axum_server::bind_rustls(addr, RustlsConfig::from_config(Arc::new(server_config)))
        .serve(app.into_make_service())
        .await
        .expect("failed to start HTTPS server");
}

pub async fn start_server_http(config_store: MemoryStringStore) -> std::io::Result<()> {
    let app = linkup_router(config_store);

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    println!("listening on {}", &addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

pub async fn start_dns_server(linkup_session_name: String, domains: Vec<String>) {
    let mut catalog = Catalog::new();

    for domain in &domains {
        let record_name = Name::from_str(&format!("{linkup_session_name}.{domain}.")).unwrap();

        let authority = InMemoryAuthority::empty(record_name.clone(), ZoneType::Primary, false);

        let record = Record::from_rdata(
            record_name.clone(),
            3600,
            RData::A(Ipv4Addr::new(127, 0, 0, 1).into()),
        );

        authority.upsert(record, 0).await;

        catalog.upsert(record_name.clone().into(), vec![Arc::new(authority)]);
    }

    let cf_name_server = NameServerConfig::new("1.1.1.1:53".parse().unwrap(), Protocol::Udp);
    let forward_config = ForwardConfig {
        name_servers: NameServerConfigGroup::from(vec![cf_name_server]),
        options: Some(ResolverOpts::default()),
    };

    let forwarder =
        ForwardAuthority::builder_with_config(forward_config, TokioConnectionProvider::default())
            .with_origin(Name::root())
            .build()
            .unwrap();

    catalog.upsert(Name::root().into(), vec![Arc::new(forwarder)]);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8053));
    let sock = UdpSocket::bind(&addr).await.unwrap();

    let mut server = ServerFuture::new(catalog);
    server.register_socket(sock);

    println!("listening on {addr}");
    server.block_until_done().await.unwrap();
}

async fn linkup_request_handler(
    Extension(store): Extension<MemoryStringStore>,
    Extension(client): Extension<HttpsClient>,
    ws: ws::ExtractOptionalWebSocketUpgrade,
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
                .unwrap_or("localhost"),
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

    match ws.0 {
        Some(downstream_upgrade) => {
            let mut url = target_service.url;
            if url.starts_with("http://") {
                url = url.replace("http://", "ws://");
            } else if url.starts_with("https://") {
                url = url.replace("https://", "wss://");
            }

            let uri = url.parse::<Uri>().unwrap();
            let host = uri.host().unwrap().to_string();
            let mut upstream_request = uri.into_client_request().unwrap();

            // Copy over all headers from the incoming request
            let mut cookie_values: Vec<String> = Vec::new();
            for (key, value) in req.headers() {
                if key == http::header::COOKIE {
                    if let Ok(cookie_value) = value.to_str().map(str::trim) {
                        if !cookie_value.is_empty() {
                            cookie_values.push(cookie_value.to_string());
                        }
                    }
                    continue;
                }

                upstream_request.headers_mut().insert(key, value.clone());
            }

            if !cookie_values.is_empty() {
                let combined = cookie_values.join("; ");
                if let Ok(cookie_header_value) = HeaderValue::from_str(&combined) {
                    upstream_request
                        .headers_mut()
                        .insert(http::header::COOKIE, cookie_header_value);
                }
            }

            linkup::normalize_cookie_header(upstream_request.headers_mut());

            // add the extra headers that linkup wants
            let extra_http_headers: HeaderMap = extra_headers.into();
            for (key, value) in extra_http_headers.iter() {
                upstream_request.headers_mut().insert(key, value.clone());
            }

            // Overriding host header neccesary for tokio_tungstenite
            upstream_request
                .headers_mut()
                .insert(http::header::HOST, HeaderValue::from_str(&host).unwrap());

            let (upstream_ws_stream, upstream_response) =
                match tokio_tungstenite::connect_async(upstream_request).await {
                    Ok(connection) => connection,
                    Err(error) => match error {
                        tokio_tungstenite::tungstenite::Error::Http(response) => {
                            let (parts, body) = response.into_parts();
                            let body = body.unwrap_or_default();

                            return Response::from_parts(parts, Body::from(body));
                        }
                        error => {
                            return Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .body(Body::from(error.to_string()))
                                .unwrap();
                        }
                    },
                };

            let mut downstream_upgrade_response =
                downstream_upgrade.on_upgrade(ws::context_handle_socket(upstream_ws_stream));

            let downstream_response_headers = downstream_upgrade_response.headers_mut();

            // The headers from the upstream response are more important - trust the upstream server
            for (upstream_key, upstream_value) in upstream_response.headers() {
                // Except for content encoding headers, cloudflare does _not_ like them..
                if !DISALLOWED_HEADERS.contains(upstream_key) {
                    downstream_response_headers
                        .insert(upstream_key.clone(), upstream_value.clone());
                }
            }

            downstream_response_headers.extend(allow_all_cors());

            downstream_upgrade_response
        }
        None => handle_http_req(req, target_service, extra_headers, client).await,
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
    linkup::normalize_cookie_header(req.headers_mut());

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

async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    println!("signal received, starting graceful shutdown");
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
