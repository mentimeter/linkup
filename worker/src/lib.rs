use regex::Regex;
use std::{collections::HashMap, sync::Arc};

use kv_store::CfWorkerStringStore;
use linkup::*;
use worker::{*, worker_sys::web_sys::console};

use futures::{
    future::{self, Either},
    stream::StreamExt,
    SinkExt,
};

mod http_util;
mod kv_store;
mod utils;

use http_util::*;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], headers: {:?}",
        Date::now().to_string(),
        req.path(),
        req.headers(),
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

async fn linkup_request_handler(mut req: Request, sessions: SessionAllocator) -> Result<Response> {
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

    if let Some(cached_response) = get_cached_req(&req, &config.cache_routes).await? {
        return Ok(cached_response);
    }

    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return plaintext_error("Bad or missing request body", 400),
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

    let mut cf_resp =
        convert_reqwest_response_to_cf(response, additional_response_headers()).await?;

    cf_resp = set_cached_req(&req, cf_resp, config.cache_routes).await?;

    Ok(cf_resp)
}

async fn connect_with_headers(url: &str, additional_headers: HashMap<String, String>) -> Result<WebSocket> {
    console_log!("url in connect: {}", url);
    let mut proper_url = match url.parse::<Url>() {
        Ok(url) => url,
        Err(_) => return Err(Error::RustError("invalid url".into())),
    };

    // With fetch we can only make requests to http(s) urls, but Workers will allow us to upgrade
    // those connections into websockets if we use the `Upgrade` header.
    let scheme: String = match proper_url.scheme() {
        "ws" => "http".into(),
        "wss" => "https".into(),
        scheme => scheme.into(),
    };

    proper_url.set_scheme(&scheme).unwrap();
    console_log!("proper url: {}", proper_url.as_str());

    let mut headers = Headers::new();
    additional_headers.iter().for_each(|(k, v)| {
        headers.append(k, v).expect("could not append header to websocket request");
    });
    headers.set("upgrade", "websocket")?;
    console_log!("headers: {:?}", headers);

    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.with_headers(headers);

    let req = Request::new_with_init(proper_url.as_str(), &init)?;
    // let mut req = Request::new(url.as_str(), Method::Get)?;
    // req.headers_mut()?.set("upgrade", "websocket")?;

    console_log!("sending request {:?}", req);
    let res = Fetch::Request(req).send().await?;
    console_log!("res {:?}", res);

    match res.websocket() {
        Some(ws) => Ok(ws),
        None => Err(Error::RustError("server did not accept".into())),
    }
}

async fn linkup_ws_handler(req: Request, sessions: SessionAllocator) -> Result<Response> {
    let url = match req.url() {
        Ok(url) => url.to_string(),
        Err(_) => return plaintext_error("Bad or missing request url", 400),
    };

    let mut headers = req
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
    headers.extend(extra_headers);

    console_log!("Connecting to {}", destination_url);
    // let dest_ws_res = WebSocket::connect(destination_url.parse()?).await;
    // let dest_ws = match dest_ws_res {
    //     Ok(ws) => ws,
    //     Err(e) => {
    //         console_log!("Failed to connect to destination: {}", e);
    //         return plaintext_error(format!("Failed to connect to destination: {}", e), 502);
    //     }
    // };

    let dest_ws_res = connect_with_headers(&destination_url, headers).await;
    let dest_ws = match dest_ws_res {
        Ok(ws) => ws,
        Err(e) => {
            console_log!("Failed to connect to destination: {}", e);
            return plaintext_error(format!("Failed to connect to destination: {}", e), 502);
        }
    };


    console_log!(" making ws pair");
    let source_ws = WebSocketPair::new()?;
    let source_ws_server = source_ws.server;

    wasm_bindgen_futures::spawn_local(async move {
        console_log!("inside spawn_local");
        let mut dest_events = dest_ws.events().expect("could not open dest event stream");
        let mut source_events = source_ws_server.events().expect("could not open source event stream");

        dest_ws.accept().expect("could not accept dest ws");
        source_ws_server.accept().expect("could not accept source ws");

        let mut error = false;

        while !error {
        match future::select(source_events.next(), dest_events.next()).await {
            Either::Left((Some(source_event), _)) => match source_event {
                Ok(WebsocketEvent::Message(msg)) => {
                    if let Some(bytes) = msg.bytes() {
                        let res = dest_ws.send_with_bytes(bytes);
                        if let Err(e) = res {
                            console_log!("Error sending to dest: {:?}", e);
                            error = true;
                        }
                    }
                }
                Ok(WebsocketEvent::Close(close)) => {
                    console_log!("Close event: {:?}", close);
                    // dest_ws.close(Some(close), None);
                    error = true;
                }
                Err(e) => {
                    console_log!("Error: {:?}", e);
                    error = true;
                }
            },
            Either::Right((Some(dest_event), _)) => match dest_event {
                Ok(WebsocketEvent::Message(msg)) => {
                    if let Some(bytes) = msg.bytes() {
                        let res = source_ws_server.send_with_bytes(bytes);
                        if let Err(e) = res {
                            console_log!("Error sending to source: {:?}", e);
                            error = true;
                        }
                    }
                }
                Ok(WebsocketEvent::Close(close)) => {
                    console_log!("Close event: {:?}", close);
                    // dest_ws.close(Some(close), None);
                    error = true;
                }
                Err(e) => {
                    console_log!("Error: {:?}", e);
                    error = true;
                }
            },
            _ => {
                console_log!("No event, error");
                error = true;
            }
        }
    }
    });

    console_log!("returning ws client");
    return Response::from_websocket(source_ws.client);
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

    if req.headers().get("upgrade").unwrap() == Some("websocket".to_string()) {
        return linkup_ws_handler(req, sessions).await;
    }

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_session_handler(req, sessions).await;
    }

    linkup_request_handler(req, sessions).await
}
