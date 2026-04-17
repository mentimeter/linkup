use axum::{
    extract::{Request, State},
    response::IntoResponse,
};
use http::{HeaderMap, StatusCode, Uri};
use linkup::{Session, SessionAllocator, get_additional_headers, get_target_service};
use worker::{Fetch, HttpResponse};

use crate::{
    http_error::HttpError, kv_store::CfWorkerStringStore, worker_state::WorkerState,
    ws::handle_ws_resp,
};

#[worker::send]
pub async fn handle_all(State(state): State<WorkerState>, mut req: Request) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let headers: linkup::HeaderMap = req.headers().into();
    let url = req.uri().to_string();
    let (session_name, config) = match sessions.get_request_session(&url, &headers).await {
        Ok(session) => session,
        Err(_) => {
            return HttpError::new(
                "Linkup was unable to determine the session origin of the request.\nMake sure your request includes a valid session ID in the referer or tracestate headers. - Worker".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
            .into_response()
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return HttpError::new(
                "The request belonged to a session, but there was no target for the request.\nCheck your routing rules in the linkup config for a match. - Worker".to_string(),
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
    linkup::normalize_cookie_header(req.headers_mut());

    let upstream_request: worker::Request = match req.try_into() {
        Ok(req) => req,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse request: {}", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let cacheable_req = is_cacheable_request(&upstream_request, &config);
    let cache_key = get_cache_key(&upstream_request, &session_name).unwrap_or_default();

    if cacheable_req && let Some(upstream_response) = get_cached_req(cache_key.clone()).await {
        let resp: HttpResponse = match upstream_response.try_into() {
            Ok(resp) => resp,
            Err(e) => {
                return HttpError::new(
                    format!("Failed to parse cached response: {}", e),
                    StatusCode::BAD_GATEWAY,
                )
                .into_response();
            }
        };

        return resp.into_response();
    }

    let mut upstream_response = match Fetch::Request(upstream_request).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to fetch from target service: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response();
        }
    };

    if is_websocket {
        handle_ws_resp(upstream_response).await.into_response()
    } else {
        if cacheable_req {
            let cache_clone = match upstream_response.cloned() {
                Ok(resp) => resp,
                Err(e) => {
                    return HttpError::new(
                        format!("Failed to clone response: {}", e),
                        StatusCode::BAD_GATEWAY,
                    )
                    .into_response();
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

        handle_http_resp(upstream_response).await.into_response()
    }
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

async fn handle_http_resp(worker_resp: worker::Response) -> impl IntoResponse {
    let mut resp: HttpResponse = match worker_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response();
        }
    };
    resp.headers_mut().extend(linkup::allow_all_cors());
    resp.into_response()
}
