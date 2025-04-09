use axum::{
    extract::{Json, Query, Request, State},
    http::StatusCode,
    middleware::{from_fn_with_state, Next},
    response::IntoResponse,
    routing::{any, get, post},
    Router,
};
use http::{HeaderMap, Uri};
use http_error::HttpError;
use kv_store::CfWorkerStringStore;
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, CreatePreviewRequest, NameKind,
    Session, SessionAllocator, UpdateSessionRequest, Version, VersionChannel,
};
use serde::{Deserialize, Serialize};
use tower_service::Service;
use worker::{
    console_error, console_log, console_warn, event, kv::KvStore, Env, Fetch, HttpRequest,
    HttpResponse,
};
use ws::handle_ws_resp;

mod http_error;
mod kv_store;
mod libdns;
mod routes;
mod tunnel;
mod ws;

const SEVEN_DAYS_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;
const MIN_SUPPORTED_CLIENT_VERSION: &str = "2.1.0";

#[derive(Clone)]
#[allow(dead_code)]
pub struct CloudflareEnvironemnt {
    account_id: String,
    tunnel_zone_id: String,
    all_zone_ids: Vec<String>,
    api_token: String,
    worker_token: String,
}

#[derive(Clone)]
pub struct LinkupState {
    pub min_supported_client_version: Version,
    pub sessions_kv: KvStore,
    pub tunnels_kv: KvStore,
    pub certs_kv: KvStore,
    pub cloudflare: CloudflareEnvironemnt,
    pub env: Env,
}

impl TryFrom<Env> for LinkupState {
    type Error = worker::Error;

    fn try_from(value: Env) -> Result<Self, Self::Error> {
        let min_supported_client_version = Version::try_from(MIN_SUPPORTED_CLIENT_VERSION)
            .expect("MIN_SUPPORTED_CLIENT_VERSION to be a valid version");

        let sessions_kv = value.kv("LINKUP_SESSIONS")?;
        let tunnels_kv = value.kv("LINKUP_TUNNELS")?;
        let certs_kv = value.kv("LINKUP_CERTIFICATE_CACHE")?;
        let cf_account_id = value.var("CLOUDFLARE_ACCOUNT_ID")?;
        let cf_tunnel_zone_id = value.var("CLOUDFLARE_TUNNEL_ZONE_ID")?;
        let cf_all_zone_ids: Vec<String> = value
            .var("CLOUDLFLARE_ALL_ZONE_IDS")?
            .to_string()
            .split(",")
            .map(String::from)
            .collect();
        let cf_api_token = value.var("CLOUDFLARE_API_TOKEN")?;
        let worker_token = value.var("WORKER_TOKEN")?;

        let state = LinkupState {
            min_supported_client_version,
            sessions_kv,
            tunnels_kv,
            certs_kv,
            cloudflare: CloudflareEnvironemnt {
                account_id: cf_account_id.to_string(),
                tunnel_zone_id: cf_tunnel_zone_id.to_string(),
                all_zone_ids: cf_all_zone_ids,
                api_token: cf_api_token.to_string(),
                worker_token: worker_token.to_string(),
            },
            env: value,
        };

        Ok(state)
    }
}

pub fn linkup_router(state: LinkupState) -> Router {
    Router::new()
        .route("/linkup/local-session", post(linkup_session_handler))
        .route("/linkup/preview-session", post(linkup_preview_handler))
        .route("/linkup/tunnel", get(get_tunnel_handler))
        .route("/linkup/check", get(always_ok))
        .route("/linkup/no-tunnel", get(no_tunnel))
        .route("/linkup", any(deprecated_linkup_session_handler))
        .merge(routes::certificate_dns::router())
        .merge(routes::certificate_cache::router())
        .route_layer(from_fn_with_state(state.clone(), authenticate))
        // Fallback for all other requests
        .fallback(any(linkup_request_handler))
        .with_state(state)
}

#[event(scheduled)]
async fn scheduled(_event: worker::ScheduledEvent, env: Env, _ctx: worker::ScheduleContext) {
    let state =
        LinkupState::try_from(env).expect("LinkupState to be buildable from worker environment");

    cleanup_unused_sessions(&state).await;
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: worker::Context,
) -> Result<axum::http::Response<axum::body::Body>, worker::Error> {
    console_error_panic_hook::set_once();

    let state = LinkupState::try_from(env)?;

    Ok(linkup_router(state).call(req).await?)
}

#[derive(Deserialize)]
struct GetTunnelParams {
    session_name: String,
}

#[derive(Serialize, Deserialize)]
struct TunnelData {
    account_id: String,
    name: String,
    url: String,
    id: String,
    secret: String,
    last_started: u64,
}

#[worker::send]
async fn get_tunnel_handler(
    State(state): State<LinkupState>,
    Query(query): Query<GetTunnelParams>,
) -> impl IntoResponse {
    let kv = state.tunnels_kv;

    let cf_client = cloudflare_client(&state.cloudflare.api_token);
    let tunnel_prefix =
        match cloudflare::linkup::tunnel_prefix(&cf_client, &state.cloudflare.tunnel_zone_id).await
        {
            Ok(prefix) => prefix,
            Err(error) => {
                console_error!("Failed resolve tunnel prefix: {}", error);

                return HttpError::new(
                    "Failed to generate tunnel.".to_string(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        };

    let tunnel_name = format!("{}{}", tunnel_prefix, query.session_name);
    let tunnel_data: Option<TunnelData> = kv.get(&tunnel_name).json().await.unwrap();

    match tunnel_data {
        Some(mut tunnel_data) => {
            tunnel_data.last_started = worker::Date::now().as_millis();
            kv.put(&tunnel_name, &tunnel_data)
                .unwrap()
                .execute()
                .await
                .unwrap();

            Json(tunnel_data).into_response()
        }
        None => {
            let tunnel_data = tunnel::create_tunnel(
                &state.cloudflare.api_token,
                &state.cloudflare.account_id,
                &state.cloudflare.tunnel_zone_id,
                &tunnel_name,
            )
            .await
            .unwrap();

            kv.put(&tunnel_name, &tunnel_data)
                .unwrap()
                .execute()
                .await
                .unwrap();

            Json(tunnel_data).into_response()
        }
    }
}

#[worker::send]
async fn linkup_session_handler(
    State(state): State<LinkupState>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let desired_name = update_req.desired_name.clone();
    let server_conf: Session = match update_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let session_name = sessions
        .store_session(server_conf, NameKind::Animal, desired_name)
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return HttpError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };

    (StatusCode::OK, name).into_response()
}

#[worker::send]
async fn linkup_preview_handler(
    State(state): State<LinkupState>,
    Json(update_req): Json<CreatePreviewRequest>,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let server_conf: Session = match update_req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - local server", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let session_name = sessions
        .store_session(server_conf, NameKind::SixChar, String::from(""))
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return HttpError::new(
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

async fn no_tunnel() -> impl IntoResponse {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        "This linkup session has no associated tunnel / was started with --no-tunnel",
    )
        .into_response()
}

#[worker::send]
async fn linkup_request_handler(
    State(state): State<LinkupState>,
    mut req: Request,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let headers: linkup::HeaderMap = req.headers().into();
    let url = req.uri().to_string();
    let (session_name, config) = match sessions.get_request_session(&url, &headers).await {
        Ok(session) => session,
        Err(_) => {
            return HttpError::new(
                "Linkup was unable to determine the session origin of the request.
                Make sure your request includes a valid session ID in the referer or tracestate headers. - Local Server".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .into_response()
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return HttpError::new(
                "The request belonged to a session, but there was no target for the request.
                Check your routing rules in the linkup config for a match. - Local Server"
                    .to_string(),
                StatusCode::NOT_FOUND,
            )
            .into_response()
        }
    };

    let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);
    let is_websocket = req
        .headers()
        .get("upgrade")
        .map(|v| v == "websocket")
        .unwrap_or(false);

    // Rewrite request for the target service
    *req.uri_mut() = Uri::try_from(target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    req.headers_mut().remove(http::header::HOST);

    let worker_req: worker::Request = match req.try_into() {
        Ok(req) => req,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse request: {}", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let cacheable_req = is_cacheable_request(&worker_req, &config);
    let cache_key = get_cache_key(&worker_req, &session_name).unwrap_or_default();
    if cacheable_req {
        if let Some(worker_resp) = get_cached_req(cache_key.clone()).await {
            let resp: HttpResponse = match worker_resp.try_into() {
                Ok(resp) => resp,
                Err(e) => {
                    return HttpError::new(
                        format!("Failed to parse cached response: {}", e),
                        StatusCode::BAD_GATEWAY,
                    )
                    .into_response()
                }
            };
            return resp.into_response();
        }
    }

    let mut worker_resp = match Fetch::Request(worker_req).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to fetch from target service: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    if is_websocket {
        handle_ws_resp(worker_resp).await.into_response()
    } else {
        if cacheable_req {
            let cache_clone = match worker_resp.cloned() {
                Ok(resp) => resp,
                Err(e) => {
                    return HttpError::new(
                        format!("Failed to clone response: {}", e),
                        StatusCode::BAD_GATEWAY,
                    )
                    .into_response()
                }
            };
            if let Err(e) = set_cached_req(cache_key, cache_clone).await {
                return HttpError::new(
                    format!("Failed to cache response: {}", e),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        }
        handle_http_resp(worker_resp).await.into_response()
    }
}

async fn cleanup_unused_sessions(state: &LinkupState) {
    let tunnels_keys = state
        .tunnels_kv
        .list()
        .limit(1000)
        .execute()
        .await
        .unwrap()
        .keys;

    let now = worker::Date::now();

    for key in tunnels_keys {
        match state.tunnels_kv.get(&key.name).json::<TunnelData>().await {
            Ok(Some(tunnel_data)) => {
                let last_started =
                    worker::Date::from(worker::DateInit::Millis(tunnel_data.last_started));

                if now.as_millis() - last_started.as_millis() > SEVEN_DAYS_MILLIS {
                    console_log!(
                        "Deleting unused tunnel '{}'. Last used: {}",
                        &key.name,
                        &last_started
                    );

                    match tunnel::delete_tunnel(
                        &state.cloudflare.api_token,
                        &state.cloudflare.account_id,
                        &state.cloudflare.tunnel_zone_id,
                        &tunnel_data.id,
                    )
                    .await
                    {
                        Ok(_) => {
                            if let Err(error) = state.tunnels_kv.delete(&key.name).await {
                                console_error!("Failed to delete tunnel info from KV: {}", error);
                            }
                        }
                        Err(error) => {
                            console_error!("Failed to delete tunnel: {}", error);
                        }
                    }
                }
            }
            Ok(None) => {
                console_warn!("Tunnel data for key '{}' not found.", &key.name);
            }
            Err(error) => {
                console_error!("Failed to deserialize tunnel data: {}", error.to_string());
            }
        }
    }
}

async fn handle_http_resp(worker_resp: worker::Response) -> impl IntoResponse {
    let mut resp: HttpResponse = match worker_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };
    resp.headers_mut().extend(allow_all_cors());
    resp.into_response()
}

fn is_cacheable_request(req: &worker::Request, config: &Session) -> bool {
    if req.method() != worker::Method::Get {
        return false;
    }
    if let Some(routes) = &config.cache_routes {
        let path = req.path();
        if routes.iter().any(|route| route.is_match(&path)) {
            return true;
        }
    }
    false
}

fn get_cache_key(req: &worker::Request, session_name: &str) -> Option<String> {
    let mut cache_url = req.url().ok()?;
    let curr_domain = cache_url.domain().unwrap_or("example.com");
    if cache_url
        .set_host(Some(&format!("{}.{}", session_name, curr_domain)))
        .is_err()
    {
        return None;
    }
    Some(cache_url.to_string())
}

async fn get_cached_req(cache_key: String) -> Option<worker::Response> {
    match worker::Cache::default().get(cache_key, false).await {
        Ok(Some(resp)) => Some(resp),
        _ => None,
    }
}

async fn set_cached_req(cache_key: String, resp: worker::Response) -> worker::Result<()> {
    // Avoid caching error statuses or partial content
    if resp.status_code() > 499 || resp.status_code() == 206 {
        return Ok(());
    }
    worker::Cache::default().put(cache_key, resp).await?;
    Ok(())
}

fn cloudflare_client(api_token: &str) -> cloudflare::framework::async_api::Client {
    cloudflare::framework::async_api::Client::new(
        cloudflare::framework::auth::Credentials::UserAuthToken {
            token: api_token.to_string(),
        },
        cloudflare::framework::HttpApiClientConfig::default(),
        cloudflare::framework::Environment::Production,
    )
    .expect("Cloudflare API Client to have been created")
}

pub fn generate_secret() -> String {
    let mut random_bytes = [0u8; 32];
    getrandom::getrandom(&mut random_bytes).unwrap();

    base64::Engine::encode(&base64::prelude::BASE64_STANDARD, random_bytes)
}

async fn authenticate(
    State(state): State<LinkupState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    if request.uri().path().starts_with("/linkup") {
        if request.uri().path() == "/linkup/local-session" {
            match headers.get("x-linkup-version") {
                Some(value) => match Version::try_from(value.to_str().unwrap()) {
                    Ok(client_version) => {
                        if client_version < state.min_supported_client_version
                            && client_version.channel() != VersionChannel::Beta
                        {
                            return (
                                    StatusCode::UNAUTHORIZED,
                                    "Your Linkup CLI is outdated, please upgrade to the latest version.",
                                )
                                    .into_response();
                        }
                    }
                    Err(_) => {
                        return (StatusCode::UNAUTHORIZED, "Invalid x-linkup-version header.")
                            .into_response();
                    }
                },
                None => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        "No x-linkup-version header, please upgrade your Linkup CLI.",
                    )
                        .into_response();
                }
            }
        }

        let authorization = headers.get(http::header::AUTHORIZATION);
        match authorization {
            Some(token) => match token.to_str() {
                Ok(token) => {
                    let parsed_token = token.replace("Bearer ", "");
                    if parsed_token != state.cloudflare.worker_token {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                }
                Err(err) => {
                    console_warn!(
                        "Token on Authorization header contains unsupported characters: '{:?}', {}",
                        token,
                        err.to_string()
                    );

                    return StatusCode::UNAUTHORIZED.into_response();
                }
            },
            None => {
                // TODO: Remove this once we've migrated all users to the new token
                return (
                    StatusCode::UNAUTHORIZED,
                    "no linkup access token provided. This token was added in linkup 2.0, check to see if your cli is up to date"
                ).into_response();
            }
        }
    }

    next.run(request).await
}

#[worker::send]
async fn deprecated_linkup_session_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        "This endpoint was deprecated in linkup 2.0, please check that your cli is up to date",
    )
        .into_response()
}
