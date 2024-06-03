use axum::{
    extract::{Json, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Router,
};

use http::{HeaderMap, Uri};
use http_error::HttpError;
use kv_store::CfWorkerStringStore;
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, NameKind, Session,
    SessionAllocator, UpdateSessionRequest,
};
use tower_service::Service;
use worker::{event, kv::KvStore, Env, Fetch, HttpRequest, HttpResponse};
use ws::handle_ws_resp;

mod http_error;
mod kv_store;
mod ws;

pub fn linkup_router(kv: KvStore) -> Router {
    Router::new()
        .route("/linkup", post(linkup_session_handler))
        .route("/preview", post(linkup_preview_handler))
        .route("/linkup-check", get(always_ok))
        .route("/linkup-no-tunnel", get(no_tunnel))
        .fallback(any(linkup_request_handler))
        .with_state(kv)
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: worker::Context,
) -> Result<axum::http::Response<axum::body::Body>, worker::Error> {
    console_error_panic_hook::set_once();

    let kv = match env.kv("LINKUP_SESSIONS") {
        Ok(kv) => kv,
        Err(e) => {
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(format!("Failed to get KV namespace: {}", e).into())
                .unwrap())
        }
    };

    Ok(linkup_router(kv).call(req).await?)
}

#[worker::send]
async fn linkup_request_handler(State(kv): State<KvStore>, mut req: Request) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(kv);
    let sessions = SessionAllocator::new(&store);

    let headers: linkup::HeaderMap = req.headers().into();
    let url = req.uri().to_string();
    let (session_name, config) = match sessions.get_request_session(&url, &headers).await {
        Ok(session) => session,
        Err(_) => {
            return HttpError::new(
                "Linkup was unable to determine the session origin of the request. Ensure that your request includes a valid session identifier in the referer or tracestate headers. - Local Server".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .into_response()
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return HttpError::new(
                "The request belonged to a session, but there was no target for the request. Check that the routing rules in your linkup config have a match for this request. - Local Server".to_string(),
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

    *req.uri_mut() = Uri::try_from(target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    // Request uri and host headers should not conflict
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
    let cache_key = get_cache_key(&worker_req, &session_name).unwrap();
    if cacheable_req {
        if let Some(worker_resp) = get_cached_req(cache_key.clone()).await {
            let resp: HttpResponse = match worker_resp.try_into() {
                Ok(resp) => resp,
                Err(e) => {
                    return HttpError::new(
                        format!("Failed to parse response: {}", e),
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

#[worker::send]
async fn linkup_session_handler(
    State(kv): State<KvStore>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(kv);
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
    State(kv): State<KvStore>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(kv);
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

fn get_cache_key(req: &worker::Request, session_name: &String) -> Option<String> {
    let mut cache_url = match req.url() {
        Ok(url) => url,
        Err(_) => return None,
    };

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
    // Cache API throws error on 206 partial content
    if resp.status_code() > 499 || resp.status_code() == 206 {
        return Ok(());
    }

    worker::Cache::default().put(cache_key, resp).await?;

    Ok(())
}
