use http_util::*;
use kv_store::CfWorkerStringStore;
use linkup::{HeaderMap as LinkupHeaderMap, *};
use regex::Regex;
use worker::*;
use ws::linkup_ws_handler;

mod http_util;
mod kv_store;
mod utils;
mod ws;

async fn linkup_session_handler<'a>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a>,
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

async fn linkup_preview_handler<'a>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a>,
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

async fn get_cached_req(
    req: &Request,
    cache_routes: &Option<Vec<Regex>>,
) -> Result<Option<Response>> {
    let path = req.path();

    if let Some(routes) = cache_routes {
        if routes.iter().any(|route| route.is_match(&path)) {
            let url = req.url()?;
            Cache::default().get(url.to_string(), false).await
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

async fn set_cached_req(
    req: &Request,
    mut resp: Response,
    cache_routes: Option<Vec<Regex>>,
) -> Result<Response> {
    if resp.status_code() != 200 {
        return Ok(resp);
    }

    let path = req.path();

    if let Some(routes) = cache_routes {
        if routes.iter().any(|route| route.is_match(&path)) {
            let url = req.url()?;
            let cache_resp = resp.cloned()?;
            Cache::default().put(url.to_string(), cache_resp).await?;

            return Ok(resp);
        }
    }

    Ok(resp)
}

async fn linkup_request_handler<'a>(
    mut req: Request,
    sessions: &'a SessionAllocator<'a>,
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

    if let Some(cached_response) = get_cached_req(&req, &config.cache_routes).await? {
        return Ok(cached_response);
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
    let client = reqwest::Client::new();
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

    cf_resp = set_cached_req(&req, cf_resp, config.cache_routes).await?;

    Ok(cf_resp)
}

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

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_session_handler(req, &sessions).await;
    }

    if req.method() == Method::Post && req.path() == "/preview" {
        return linkup_preview_handler(req, &sessions).await;
    }

    linkup_request_handler(req, &sessions).await
}
