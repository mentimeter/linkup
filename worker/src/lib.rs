use axum::{
    extract::{Json, Query, Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get, post},
    Router,
};
use http::{HeaderMap, Uri};
use http_error::HttpError;
use kv_store::CfWorkerStringStore;
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, CreatePreviewRequest, NameKind,
    Session, SessionAllocator, UpdateSessionRequest,
};
use serde::{Deserialize, Serialize};
use tower_service::Service;
use worker::{event, kv::KvStore, Env, Fetch, HttpRequest, HttpResponse};
use ws::handle_ws_resp;

mod cloudflare;
mod http_error;
mod kv_store;
mod tunnel;
mod ws;

#[derive(Clone)]
#[allow(dead_code)]
pub struct CloudflareEnvironemnt {
    account_id: String,
    tunnel_zone_id: String,
    all_zone_ids: Vec<String>,
    api_token: String,
}

#[derive(Clone)]
pub struct LinkupState {
    pub sessions_kv: KvStore,
    pub tunnels_kv: KvStore,
    pub certs_kv: KvStore,
    pub cloudflare: CloudflareEnvironemnt,
}

pub fn linkup_router(state: LinkupState) -> Router {
    Router::new()
        .route("/linkup/local-session", post(linkup_session_handler))
        .route("/linkup/preview-session", post(linkup_preview_handler))
        .route("/linkup/tunnel", get(get_tunnel_handler))
        .route("/linkup/check", get(always_ok))
        .route("/linkup/no-tunnel", get(no_tunnel))
        .route(
            "/linkup/certificate-dns",
            get(get_certificate_dns_handler)
                .post(create_certificate_dns_handler)
                .put(update_certificate_dns_handler)
                .delete(delete_certificate_dns_handler),
        )
        .route(
            "/linkup/certificate-cache",
            get(get_certificate_cache_handler)
                .post(create_certificate_cache_handler)
                .put(update_certificate_cache_handler)
                .delete(delete_certificate_cache_handler),
        )
        // Fallback for all other requests
        .fallback(any(linkup_request_handler))
        .with_state(state)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: worker::Context,
) -> Result<axum::http::Response<axum::body::Body>, worker::Error> {
    console_error_panic_hook::set_once();

    let sessions_kv = env.kv("LINKUP_SESSIONS")?;
    let tunnels_kv = env.kv("LINKUP_TUNNELS")?;
    let certs_kv = env.kv("LINKUP_CERTIFICATE_CACHE")?;
    let cf_account_id = env.var("CLOUDFLARE_ACCOUNT_ID")?;
    let cf_tunnel_zone_id = env.var("CLOUDFLARE_TUNNEL_ZONE_ID")?;
    let cf_all_zone_ids: Vec<String> = env
        .var("CLOUDLFLARE_ALL_ZONE_IDS")?
        .to_string()
        .split(",")
        .map(String::from)
        .collect();
    let cf_api_token = env.var("CLOUDFLARE_API_TOKEN")?;

    let state = LinkupState {
        sessions_kv,
        tunnels_kv,
        certs_kv,
        cloudflare: CloudflareEnvironemnt {
            account_id: cf_account_id.to_string(),
            tunnel_zone_id: cf_tunnel_zone_id.to_string(),
            all_zone_ids: cf_all_zone_ids,
            api_token: cf_api_token.to_string(),
        },
    };

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
    let tunnel_name = format!("linkup-tunnel-{}", query.session_name);
    let tunnel_data: Option<TunnelData> = kv.get(&tunnel_name).json().await.unwrap();

    match tunnel_data {
        Some(tunnel_data) => {
            // TODO: Update the last_started field to `now`.
            return Json(tunnel_data);
        }
        None => {
            let (tunnel_id, tunnel_secret) = tunnel::create_tunnel(
                &state.cloudflare.api_token,
                &state.cloudflare.account_id,
                &tunnel_name,
            )
            .await
            .unwrap();

            tunnel::create_dns_record(
                &state.cloudflare.api_token,
                &state.cloudflare.tunnel_zone_id,
                &tunnel_id,
                &tunnel_name,
            )
            .await
            .unwrap();

            let zone_domain = tunnel::get_zone_domain(
                &state.cloudflare.api_token,
                &state.cloudflare.tunnel_zone_id,
            )
            .await;

            let tunnel_data = TunnelData {
                account_id: state.cloudflare.account_id,
                name: tunnel_name.clone(),
                url: format!("https://{}.{}", &tunnel_name, &zone_domain),
                id: tunnel_id,
                secret: tunnel_secret,
                last_started: worker::Date::now().as_millis(),
            };

            kv.put(&tunnel_name, &tunnel_data)
                .unwrap()
                .execute()
                .await
                .unwrap();

            Json(tunnel_data)
        }
    }
}

#[worker::send]
async fn get_certificate_dns_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "get_certificate_dns_handler stub").into_response()
}

#[worker::send]
async fn create_certificate_dns_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("create_certificate_dns_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn update_certificate_dns_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("update_certificate_dns_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn delete_certificate_dns_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "delete_certificate_dns_handler stub").into_response()
}

#[worker::send]
async fn get_certificate_cache_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "get_certificate_cache_handler stub").into_response()
}

#[worker::send]
async fn create_certificate_cache_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("create_certificate_cache_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn update_certificate_cache_handler(
    State(_state): State<LinkupState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        format!("update_certificate_cache_handler stub: {:?}", payload),
    )
}

#[worker::send]
async fn delete_certificate_cache_handler(State(_state): State<LinkupState>) -> impl IntoResponse {
    (StatusCode::OK, "delete_certificate_cache_handler stub").into_response()
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
