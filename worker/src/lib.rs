use std::{collections::HashMap, sync::Arc};

use kv_store::CfWorkerStringStore;
use linkup::*;
use worker::*;

mod http_util;
mod kv_store;
mod utils;

use http_util::*;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or_else(|| "unknown region".into())
    );
}

fn plaintext_error(msg: impl Into<String>, status: u16) -> Result<Response> {
    let mut resp = Response::error(msg, status).unwrap();
    let headers = resp.headers_mut();
    _ = headers.set("Content-Type", "text/plain");

    Ok(resp)
}

async fn linkup_session_handler(mut req: Request, sessions: SessionAllocator) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
    };

    let input_yaml_conf = match String::from_utf8(body_bytes) {
        Ok(input_yaml_conf) => input_yaml_conf,
        Err(_) => return plaintext_error("Invalid request body encoding", 400),
    };

    match update_session_req_from_json(input_yaml_conf) {
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

async fn linkup_request_handler(mut req: Request, sessions: SessionAllocator) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
    };

    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return plaintext_error("Bad or missing request url", 400),
    };

    let headers = req
        .headers()
        .clone()
        .entries()
        .collect::<HashMap<String, String>>();

    let (session_name, config) =
        match sessions.get_request_session(url.clone(), headers.clone()).await {
            Ok(result) => result,
            Err(_) => return plaintext_error("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate", 422),
        };

    let destination_url = match get_target_url(url.clone(), headers.clone(), &config, &session_name)
    {
        Some(result) => result,
        None => return plaintext_error("No target URL for request", 422),
    };

    let extra_headers = get_additional_headers(url, &headers, &session_name);

    let method = match convert_cf_method_to_reqwest(&req.method()) {
        Ok(method) => method,
        Err(_) => return plaintext_error("Bad request method", 400),
    };

    // // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::new();
    let response_result = client
        .request(method, &destination_url)
        .headers(merge_headers(headers, extra_headers))
        .body(body_bytes)
        .send()
        .await;

    let response = match response_result {
        Ok(response) => response,
        Err(e) => return plaintext_error(format!("Failed to proxy request: {}", e), 502),
    };

    convert_reqwest_response_to_cf(response, common_response_headers()).await
}

async fn linkup_ws_handler(req: Request, sessions: SessionAllocator) -> Result<Response> {
    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return plaintext_error("Bad or missing request url", 400),
    };

    let headers = req
        .headers()
        .clone()
        .entries()
        .collect::<HashMap<String, String>>();

    let (session_name, config) =
        match sessions.get_request_session(url.clone(), headers.clone()).await {
            Ok(result) => result,
            Err(_) => return plaintext_error("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate", 422),
        };

    let destination_url = match get_target_url(url.clone(), headers.clone(), &config, &session_name)
    {
        Some(result) => result,
        None => return plaintext_error("No target URL for request", 422),
    };

    let redirect_dest = Url::parse(&destination_url)?;

    Response::redirect(redirect_dest)
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    let kv = match env.kv("LINKUP_SESSIONS") {
        Ok(kv) => kv,
        Err(e) => return plaintext_error(format!("Failed to get KV store: {}", e), 500),
    };

    let string_store = CfWorkerStringStore::new(kv);

    let sessions = SessionAllocator::new(Arc::new(string_store));

    // if req.headers().get("upgrade").unwrap() == Some("websocket".to_string()) {
    //     return linkup_ws_handler(req, sessions).await;
    // }

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_session_handler(req, sessions).await;
    }

    linkup_request_handler(req, sessions).await
}
