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

async fn linkup_session_handler(mut req: Request, sessions: SessionAllocator) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Response::error("Bad or missing request body", 400),
    };

    let input_yaml_conf = match String::from_utf8(body_bytes) {
        Ok(input_yaml_conf) => input_yaml_conf,
        Err(_) => return Response::error("Invalid request body encoding", 400),
    };

    match update_session_req_from_json(input_yaml_conf) {
        Ok((desired_name, server_conf)) => {
            let session_name = sessions
                .store_session(server_conf, NameKind::Animal, desired_name)
                .await;

            match session_name {
                Ok(session_name) => Response::ok(session_name),
                Err(e) => Response::error(format!("Failed to store server config: {}", e), 500),
            }
        }
        Err(e) => Response::error(format!("Failed to parse server config: {}", e), 400),
    }
}

async fn linkup_request_handler(mut req: Request, sessions: SessionAllocator) -> Result<Response> {
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return Response::error("Bad or missing request body", 400),
    };

    let body = if !body_bytes.is_empty() {
        let body_string = match String::from_utf8(body_bytes) {
            Ok(body_string) => body_string,
            Err(_) => return Response::error("Invalid request body encoding", 400),
        };
        Some(wasm_bindgen::JsValue::from_str(&body_string))
    } else {
        None
    };

    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return Response::error("Bad or missing request url", 400),
    };

    let headers = req
        .headers()
        .clone()
        .entries()
        .collect::<HashMap<String, String>>();

    let (session_name, config) =
        match sessions.get_request_session(url.clone(), headers.clone()).await {
            Ok(result) => result,
            Err(_) => return Response::error("Could not find a linkup session for this request. Use a linkup subdomain or context headers like Referer/tracestate", 422),
        };

    let (destination_url, service) =
        match get_target_url(url.clone(), headers.clone(), &config, &session_name) {
            Some(result) => result,
            None => return Response::error("No target URL for request", 422),
        };

    let extra_headers = get_additional_headers(url, &headers, &session_name, &service);

    let mut dest_req_init = RequestInit::new();
    dest_req_init.with_method(req.method());

    let new_headers = match merge_headers(headers, extra_headers) {
        Ok(headers) => headers,
        Err(e) => return Response::error(format!("Failed to merge headers: {}", e), 500),
    };
    dest_req_init.with_headers(new_headers);

    dest_req_init.with_body(body);

    let destination_req = match Request::new_with_init(&destination_url, &dest_req_init) {
        Ok(req) => req,
        Err(e) => {
            console_log!("Failed to create destination request: {}", e);
            return Response::error("Failed to create destination request", 500);
        }
    };

    Fetch::Request(destination_req).send().await
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

    let string_store = CfWorkerStringStore::new(kv);

    let sessions = SessionAllocator::new(Arc::new(string_store));

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_session_handler(req, sessions).await;
    }

    linkup_request_handler(req, sessions).await
}
