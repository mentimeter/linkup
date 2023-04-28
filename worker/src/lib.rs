use std::collections::HashMap;

use kv_store::KvSessionStore;
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

async fn linkup_config_handler(mut req: Request, store: KvSessionStore) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Response::error("Bad or missing request body", 400),
    };

    let input_yaml_conf = match String::from_utf8(body_bytes) {
        Ok(input_yaml_conf) => input_yaml_conf,
        Err(_) => return Response::error("Invalid request body encoding", 400),
    };

    match new_server_config_post(input_yaml_conf) {
        Ok((desired_name, server_conf)) => {
            let session_name = store.new_session(server_conf, NameKind::Animal, Some(desired_name)).await;
            Response::ok(session_name)
        }
        Err(e) => Response::error(format!("Failed to parse server config: {}", e), 400),
    }
}

async fn linkup_request_handler(mut req: Request, store: KvSessionStore) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Response::error("Bad or missing request body", 400),
    };

    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return Response::error("Bad or missing request url", 400),
    };

    let headers = req.headers().entries().collect::<HashMap<String, String>>();

    let (session_name, config) =
        match async_get_request_session(url.clone(), headers.clone(), |n| store.get(n)).await {
            Ok(result) => result,
            Err(_) => return Response::error("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate", 422),
        };

    let (destination_url, service) =
        match get_target_url(url.clone(), headers.clone(), &config, &session_name) {
            Some(result) => result,
            None => return Response::error("No target URL for request", 422),
        };

    let extra_headers = get_additional_headers(url, &headers, &session_name, &service);

    let method = match convert_cf_method_to_reqwest(&req.method()) {
        Ok(method) => method,
        Err(_) => return Response::error("Bad request method", 400),
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
        Err(e) => return Response::error(format!("Failed to proxy request: {}", e), 502),
    };

    convert_reqwest_response_to_cf(response).await
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    let kv = match env.kv("LINKUP_SESSIONS") {
        Ok(kv) => kv,
        Err(e) => return Response::error(format!("Failed to get KV store: {}", e), 500),
    };

    let store = KvSessionStore::new(kv);

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_config_handler(req, store).await;
    }

    linkup_request_handler(req, store).await
}
