use std::sync::Arc;

use axum::{
    extract::{Json, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Extension, Router,
};
// use futures::stream::StreamExt;
// use futures::TryStreamExt;
// use http_util::*;
use kv_store::CfWorkerStringStore;
// use linkup::{HeaderMap as LinkupHeaderMap, *};
// use tower_service::Service;
// use kv::KvStore;
use linkup::{NameKind, Session, SessionAllocator, UpdateSessionRequest};
use tower_service::Service;
use worker::{event, kv::KvStore, Env, HttpRequest};
// use ws::linkup_ws_handler;

// mod http_util;
mod kv_store;
// mod utils;
// mod ws;

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

// #[derive(Clone)]
// struct AppState {
//     kv: Arc<KvStore>,
// }

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        Response::builder()
            .status(self.status_code)
            .header("Content-Type", "text/plain")
            .body(axum::body::Body::from(self.message))
            .unwrap()
    }
}

pub fn linkup_router(kv: KvStore) -> Router {
    // let state = AppState { kv: Arc::new(kv) };
    Router::new()
        .route("/linkup", post(linkup_session_handler))
        .route("/linkup-check", get(always_ok))
        // .fallback(any(linkup_request_handler))
        // .layer(Extension(env))
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

// Ok(linkup_router(kv).call(req).await?)

// return match (req.method(), req.uri().path()) {
//     (&Method::POST, "/linkup") => linkup_session_handler(req, &sessions).await,
//     (&Method::POST, "/preview") => linkup_preview_handler(req, &sessions).await,
//     (&Method::GET, "/linkup-no-tunnel") => plaintext_error(
//         "This linkup session has no associated tunnel / was started with --no-tunnel",
//         422,
//     ),
//     _ => linkup_request_handler(req, &sessions).await,
// };
// }

async fn always_ok() -> &'static str {
    "OK"
}

async fn linkup_session_handler(
    State(linkupState): State<KvStore>,
    Json(update_req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(linkupState);
    let sessions = SessionAllocator::new(&store);

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

// async fn linkup_preview_handler(
//     Extension(env): Extension<Env>,
//     Json(update_req): Json<UpdateSessionRequest>,
// ) -> impl IntoResponse {
//     let store = match store_from_env(&env) {
//         Ok(store) => store,
//         Err(e) => return e.into_response(),
//     };
//     let sessions = SessionAllocator::new(&store);

//     let server_conf: Session = match update_req.try_into() {
//         Ok(conf) => conf,
//         Err(e) => {
//             return ApiError::new(
//                 format!("Failed to parse server config: {} - local server", e),
//                 StatusCode::BAD_REQUEST,
//             )
//             .into_response()
//         }
//     };

//     let session_name = sessions
//         .store_session(server_conf, NameKind::SixChar, String::from(""))
//         .await;

//     let name = match session_name {
//         Ok(session_name) => session_name,
//         Err(e) => {
//             return ApiError::new(
//                 format!("Failed to store server config: {}", e),
//                 StatusCode::INTERNAL_SERVER_ERROR,
//             )
//             .into_response()
//         }
//     };

//     (StatusCode::OK, name).into_response()
// }

// fn store_from_env(env: &Env) -> Result<CfWorkerStringStore, ApiError> {
//     let kv = match env.kv("LINKUP_SESSIONS") {
//         Ok(kv) => kv,
//         Err(e) => {
//             return Err(ApiError::new(
//                 format!("Failed to get KV namespace: {}", e),
//                 StatusCode::INTERNAL_SERVER_ERROR,
//             ));
//         }
//     };

//     Ok()
// }

// async fn linkup_request_handler<'a, S: StringStore>(
//     mut req: Request,
//     sessions: &'a SessionAllocator<'a, S>,
// ) -> Result<Response> {
//     let url = match req.url() {
//         Ok(url) => url.to_string(),
//         Err(_) => return plaintext_error("Bad or missing request url", 400),
//     };

//     let mut headers = LinkupHeaderMap::from_worker_request(&req);

//     let (session_name, config) =
//         match sessions.get_request_session(&url, &headers).await {
//             Ok(result) => result,
//             Err(e) => return plaintext_error(format!("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate, {:?}",e), 422),
//         };

//     if is_cacheable_request(&req, &config) {
//         if let Some(cached_response) = get_cached_req(&req, &session_name).await {
//             return Ok(cached_response);
//         }
//     }

//     let body_bytes = match req.bytes().await {
//         Ok(bytes) => bytes,
//         Err(_) => return plaintext_error("Bad or missing request body", 400),
//     };

//     let target_service = match get_target_service(&url, &headers, &config, &session_name) {
//         Some(result) => result,
//         None => return plaintext_error("No target URL for request", 422),
//     };

//     let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);

//     let method = match convert_cf_method_to_reqwest(&req.method()) {
//         Ok(method) => method,
//         Err(_) => return plaintext_error("Bad request method", 400),
//     };

//     // if let Ok(Some(upgrade)) = req.headers().get("upgrade") {
//     //     if upgrade == "websocket" {
//     //         return linkup_ws_handler(req, &sessions).await;
//     //     }
//     // }

//     // // Proxy the request using the destination_url and the merged headers
//     let client = reqwest::Client::builder()
//         .redirect(reqwest::redirect::Policy::none())
//         .build()
//         .unwrap();

//     headers.extend(&extra_headers);
//     let response_result = client
//         .request(method, &target_service.url)
//         .headers(headers.into())
//         .body(body_bytes)
//         .send()
//         .await;

//     let response = match response_result {
//         Ok(response) => response,
//         Err(e) => return plaintext_error(format!("Failed to proxy request: {}", e), 502),
//     };

//     let mut cf_resp =
//         convert_reqwest_response_to_cf(response, &additional_response_headers()).await?;

//     if is_cacheable_request(&req, &config) {
//         cf_resp = set_cached_req(&req, cf_resp, session_name).await?;
//     }

//     Ok(cf_resp)
// }

// fn is_cacheable_request(req: &Request, config: &Session) -> bool {
//     if req.method() != Method::Get {
//         return false;
//     }

//     if let Some(routes) = &config.cache_routes {
//         let path = req.path();
//         if routes.iter().any(|route| route.is_match(&path)) {
//             return true;
//         }
//     }

//     false
// }

// fn get_cache_key(req: &Request, session_name: &String) -> Option<String> {
//     let mut cache_url = match req.url() {
//         Ok(url) => url,
//         Err(_) => return None,
//     };

//     let curr_domain = cache_url.domain().unwrap_or("example.com");
//     if cache_url
//         .set_host(Some(&format!("{}.{}", session_name, curr_domain)))
//         .is_err()
//     {
//         return None;
//     }

//     Some(cache_url.to_string())
// }

// async fn get_cached_req(req: &Request, session_name: &String) -> Option<Response> {
//     let cache_key = match get_cache_key(req, session_name) {
//         Some(cache_key) => cache_key,
//         None => return None,
//     };

//     match Cache::default().get(cache_key, false).await {
//         Ok(Some(resp)) => Some(resp),
//         _ => None,
//     }
// }

// async fn set_cached_req(
//     req: &Request,
//     mut resp: Response,
//     session_name: String,
// ) -> Result<Response> {
//     // Cache API throws error on 206 partial content
//     if resp.status_code() > 499 || resp.status_code() == 206 {
//         return Ok(resp);
//     }

//     if let Some(cache_key) = get_cache_key(req, &session_name) {
//         let cache_resp = resp.cloned()?;
//         Cache::default().put(cache_key, cache_resp).await?;
//     }

//     Ok(resp)
// }
