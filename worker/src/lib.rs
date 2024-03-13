use http_util::*;
use kv_store::CfWorkerStringStore;
use linkup::{HeaderMap as LinkupHeaderMap, *};
use worker::*;
use ws::linkup_ws_handler;

mod http_util;
mod kv_store;
mod utils;
mod ws;

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    let kv = match env.kv("LINKUP_SESSIONS") {
        Ok(kv) => kv,
        Err(e) => return plaintext_error(format!("Failed to get KV store: {}", e), 500),
    };

    let string_store = CfWorkerStringStore::new(kv);

    let sessions = SessionAllocator::new(&string_store);

    if let Ok(Some(upgrade)) = req.headers().get("upgrade") {
        if upgrade == "websocket" {
            return linkup_ws_handler(req, &sessions).await;
        }
    }

    return match (req.method(), req.path().as_str()) {
        (Method::Post, "/linkup") => linkup_session_handler(req, &sessions).await,
        (Method::Post, "/preview") => linkup_preview_handler(req, &sessions).await,
        (Method::Get, "/linkup-no-tunnel") => plaintext_error(
            "This linkup session has no associated tunnel / was started with --no-tunnel",
            422,
        ),
        _ => linkup_request_handler(req, &sessions).await,
    };
}

async fn linkup_session_handler<'a, S: StringStore>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a, S>,
) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
    };

    let input_json_conf = match String::from_utf8(body_bytes) {
        Ok(input_json_conf) => input_json_conf,
        Err(_) => return plaintext_error("Invalid request body encoding", 400),
    };

    match update_session_req_from_json(input_json_conf) {
        Ok((desired_name, server_conf)) => {
            let session_name = sessions
                .store_session(server_conf, NameKind::Animal, desired_name)
                .await;

            match session_name {
                Ok(session_name) => Response::ok(session_name),
                Err(e) => plaintext_error(format!("Failed to store server config: {}", e), 500),
            }
        }
        Err(e) => plaintext_error(format!("Failed to parse server config: {}", e), 400),
    }
}

async fn linkup_preview_handler<'a, S: StringStore>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a, S>,
) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
    };

    let input_json_conf = match String::from_utf8(body_bytes) {
        Ok(input_json_conf) => input_json_conf,
        Err(_) => return plaintext_error("Invalid request body encoding", 400),
    };

    match create_preview_req_from_json(input_json_conf) {
        Ok(preview) => {
            let session_name = sessions
                .store_session(preview, NameKind::SixChar, String::from(""))
                .await;

            match session_name {
                Ok(session_name) => Response::ok(session_name),
                Err(e) => plaintext_error(format!("Failed to store server config: {}", e), 500),
            }
        }
        Err(e) => plaintext_error(format!("Failed to parse server config: {}", e), 400),
    }
}

async fn linkup_request_handler<'a, S: StringStore>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a, S>,
) -> Result<Response> {
    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return plaintext_error("Bad or missing request url", 400),
    };

    let mut headers = LinkupHeaderMap::from_worker_request(&req);

    let (session_name, config) =
        match sessions.get_request_session(&url, &headers).await {
            Ok(result) => result,
            Err(e) => return plaintext_error(format!("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate, {:?}",e), 422),
        };

    if is_cacheable_request(&req, &config) {
        if let Some(cached_response) = get_cached_req(&req, &session_name).await {
            return Ok(cached_response);
        }
    }

    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => return plaintext_error("No target URL for request", 422),
    };

    let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);

    let method = match convert_cf_method_to_reqwest(&req.method()) {
        Ok(method) => method,
        Err(_) => return plaintext_error("Bad request method", 400),
    };

    // // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    headers.extend(&extra_headers);
    let response_result = client
        .request(method, &target_service.url)
        .headers(headers.into())
        .body(body_bytes)
        .send()
        .await;

    let response = match response_result {
        Ok(response) => response,
        Err(e) => return plaintext_error(format!("Failed to proxy request: {}", e), 502),
    };

    let mut cf_resp =
        convert_reqwest_response_to_cf(response, &additional_response_headers()).await?;

    if is_cacheable_request(&req, &config) {
        cf_resp = set_cached_req(&req, cf_resp, session_name).await?;
    }

    Ok(cf_resp)
}

fn is_cacheable_request(req: &Request, config: &Session) -> bool {
    if req.method() != Method::Get {
        return false;
    }

    if config.session_token == PREVIEW_SESSION_TOKEN {
        return true;
    }

    if let Some(routes) = &config.cache_routes {
        let path = req.path();
        if routes.iter().any(|route| route.is_match(&path)) {
            return true;
        }
    }

    false
}

fn get_cache_key(req: &Request, session_name: &String) -> Option<String> {
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

async fn get_cached_req(req: &Request, session_name: &String) -> Option<Response> {
    let cache_key = match get_cache_key(req, session_name) {
        Some(cache_key) => cache_key,
        None => return None,
    };

    match Cache::default().get(cache_key, false).await {
        Ok(Some(resp)) => Some(resp),
        _ => None,
    }
}

async fn set_cached_req(
    req: &Request,
    mut resp: Response,
    session_name: String,
) -> Result<Response> {
    // Cache API throws error on 206 partial content
    if resp.status_code() > 499 || resp.status_code() == 206 {
        return Ok(resp);
    }

    if let Some(cache_key) = get_cache_key(req, &session_name) {
        let cache_resp = resp.cloned()?;
        Cache::default().put(cache_key, cache_resp).await?;
    }

    Ok(resp)
}
