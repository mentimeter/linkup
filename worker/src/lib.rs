use std::sync::Arc;

use axum::{
    debug_handler,
    extract::{Json, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Router,
};
use futures::{
    future::{self, Either},
    stream::StreamExt,
};
use http::{HeaderMap, Uri};
use kv_store::CfWorkerStringStore;
// use linkup::{HeaderMap as LinkupHeaderMap, *};
// use tower_service::Service;
// use kv::KvStore;
use linkup::{
    allow_all_cors, get_additional_headers, get_target_service, NameKind, Session,
    SessionAllocator, UpdateSessionRequest,
};
use tower_service::Service;
use worker::{
    console_log, event, kv::KvStore, Env, Error, Fetch, HttpRequest, HttpResponse, WebSocketPair,
};
// use ws::linkup_ws_handler;
use ws::{close_with_internal_error, forward_ws_event};

// mod http_util;
mod kv_store;
// mod utils;
mod ws;

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
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        Response::builder()
            .status(self.status_code)
            .header("Content-Type", "text/plain")
            .body(axum::body::Body::from(self.message))
            .unwrap()
    }
}

pub fn linkup_router(kv: KvStore) -> Router {
    Router::new()
        .route("/linkup", post(linkup_session_handler))
        .route("/linkup-check", get(always_ok))
        .fallback(any(linkup_request_handler))
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

#[debug_handler]
#[worker::send]
async fn linkup_request_handler(State(kv): State<KvStore>, req: Request) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(kv);
    let sessions = SessionAllocator::new(&store);

    let headers: linkup::HeaderMap = req.headers().into();
    let url = req.uri().to_string();
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
        handle_ws_req(req, target_service, extra_headers)
            .await
            .into_response()
    } else {
        handle_http_req(req, target_service, extra_headers)
            .await
            .into_response()
    }
}

async fn handle_http_req(
    mut req: Request,
    target_service: linkup::TargetService,
    extra_headers: linkup::HeaderMap,
) -> impl IntoResponse {
    *req.uri_mut() = Uri::try_from(target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    // Request uri and host headers should not conflict
    req.headers_mut().remove(http::header::HOST);

    let worker_req: worker::Request = match req.try_into() {
        Ok(req) => req,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse request: {}", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let mut worker_resp = match Fetch::Request(worker_req).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!("Failed to fetch from target service: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let mut resp: HttpResponse = match worker_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    resp.headers_mut().extend(allow_all_cors());

    resp.into_response()
}

async fn handle_ws_req(
    mut req: Request,
    target_service: linkup::TargetService,
    extra_headers: linkup::HeaderMap,
) -> impl IntoResponse {
    *req.uri_mut() = Uri::try_from(target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    // Request uri and host headers should not conflict
    req.headers_mut().remove(http::header::HOST);

    let worker_req: worker::Request = match req.try_into() {
        Ok(req) => req,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse request: {}", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let mut worker_resp = match Fetch::Request(worker_req).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!("Failed to fetch from target service: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let dest_ws_res = match worker_resp.websocket() {
        Some(ws) => Ok(ws),
        None => Err(Error::RustError("server did not accept".into())),
    };
    let dest_ws = match dest_ws_res {
        Ok(ws) => ws,
        Err(e) => {
            return ApiError::new(
                format!("Failed to connect to destination: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let source_ws = match WebSocketPair::new() {
        Ok(ws) => ws,
        Err(e) => {
            return ApiError::new(
                format!("Failed to create source websocket: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };
    let source_ws_server = source_ws.server;

    wasm_bindgen_futures::spawn_local(async move {
        let mut dest_events = dest_ws.events().expect("could not open dest event stream");
        let mut source_events = source_ws_server
            .events()
            .expect("could not open source event stream");

        dest_ws.accept().expect("could not accept dest ws");
        source_ws_server
            .accept()
            .expect("could not accept source ws");

        loop {
            match future::select(source_events.next(), dest_events.next()).await {
                Either::Left((Some(source_event), _)) => {
                    if let Err(e) = forward_ws_event(
                        source_event,
                        &source_ws_server,
                        &dest_ws,
                        "to destination".into(),
                    ) {
                        console_log!("Error forwarding source event: {:?}", e);
                        break;
                    }
                }
                Either::Right((Some(dest_event), _)) => {
                    if let Err(e) = forward_ws_event(
                        dest_event,
                        &dest_ws,
                        &source_ws_server,
                        "to source".into(),
                    ) {
                        console_log!("Error forwarding dest event: {:?}", e);
                        break;
                    }
                }
                _ => {
                    console_log!("No event received, error");
                    close_with_internal_error(
                        "Received something other than event from streams".to_string(),
                        &source_ws_server,
                        &dest_ws,
                    );
                    break;
                }
            }
        }
    });

    let worker_resp = match worker::Response::from_websocket(source_ws.client) {
        Ok(res) => res,
        Err(e) => {
            return ApiError::new(
                format!("Failed to create response from websocket: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };
    let mut resp: HttpResponse = match worker_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    resp.headers_mut().extend(allow_all_cors());

    resp.into_response()
}

// async fn websocket_connect(url: &str, additional_headers: &LinkupHeaderMap) -> Result<WebSocket> {
//     let mut proper_url = match url.parse::<Url>() {
//         Ok(url) => url,
//         Err(_) => return Err(Error::RustError("invalid url".into())),
//     };

//     // With fetch we can only make requests to http(s) urls, but Workers will allow us to upgrade
//     // those connections into websockets if we use the `Upgrade` header.
//     let scheme: String = match proper_url.scheme() {
//         "ws" => "http".into(),
//         "wss" => "https".into(),
//         scheme => scheme.into(),
//     };

//     proper_url.set_scheme(&scheme).unwrap();

//     let mut headers = worker::Headers::new();
//     additional_headers.into_iter().for_each(|(k, v)| {
//         headers
//             .append(k.as_str(), v.as_str())
//             .expect("could not append header to websocket request");
//     });
//     headers.set("upgrade", "websocket")?;

//     let mut init = RequestInit::new();
//     init.with_method(Method::Get);
//     init.with_headers(headers);

//     let req = Request::new_with_init(proper_url.as_str(), &init)?;

//     let res = Fetch::Request(req).send().await?;

//     match res.websocket() {
//         Some(ws) => Ok(ws),
//         None => Err(Error::RustError("server did not accept".into())),
//     }
// }

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

async fn always_ok() -> &'static str {
    "OK"
}

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
