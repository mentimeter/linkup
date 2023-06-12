use regex::Regex;
// use tokio_tungstenite::tungstenite::{
//     Message,
// };
use std::{collections::HashMap, sync::Arc};
use reqwest::{Response as ReqwestResponse };

use futures::{
    future::{self, Either},
    stream::StreamExt,
    SinkExt,
};
use kv_store::CfWorkerStringStore;
use linkup::*;
use worker::*;

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


    async fn my_connect(url: &str) -> Result<WebSocket> {
        // self.0.connect()

        let mut headers = Headers::new();
        headers.append("upgrade", "websocket")?;

        let mut init = RequestInit::new();
        init.with_method(Method::Get);
        init.with_headers(headers);

        let req = Request::new_with_init(url, &init)?;
        // let mut req = Request::new(url.as_str(), Method::Get)?;
        // req.headers_mut()?.set("upgrade", "websocket")?;

        let res = Fetch::Request(req).send().await?;

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

    let thing_ws = my_connect("https://socketsbay.com/wss/v2/1/demo/").await?;
    console_log!("thing_ws: {:?}", thing_ws);

    return Response::ok("okokokok");

    let extra_headers = get_additional_headers(url, &headers, &session_name);
    let method = match convert_cf_method_to_reqwest(&req.method()) {
        Ok(method) => method,
        Err(_) => return plaintext_error("Bad request method", 400),
    };

    // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::new();
    let response_result = client
        .request(method, destination_url)
        .headers(merge_headers(headers, extra_headers))
        .send()
        .await;

        let response: ReqwestResponse =
        match response_result {
            Ok(response) => response,
            Err(_) => return plaintext_error("Failed to proxy websocket request", 502),        };


     // Make sure the server is willing to accept the websocket.
     let status = response.status().as_u16();
     if status != 101 {
         return plaintext_error("Underlying server did not accept websocket", 503)   
    }

    // response.upgrade()
    // let upgrade_result = response.upgrade().await;
    // let upgrade = match upgrade_result {
    //     Ok(response) => response,
    //     Err(_) => {
    //         return plaintext_error("Failed to upgrade websocket", 502)
    //     }
    // };

    let source_ws = WebSocketPair::new()?;
    let source_ws_server = source_ws.server;
    source_ws_server.accept()?;

    wasm_bindgen_futures::spawn_local(async move {
        let mut source_server_events = source_ws_server.events().expect("could not open event stream");
    //     let websocket = tokio_tungstenite::WebSocketStream::from_raw_socket(
    //         upgrade,
    //         tokio_tungstenite::tungstenite::protocol::Role::Client,
    //         None
    //     ).await;
    //     let (mut dest_write, mut dest_read) = websocket.split();

    //     let mut error = false;

    //     while !error {
    //     match future::select(source_server_events.next(), dest_read.next()).await {
    //         Either::Left((Some(source_event), _)) => match source_event {
    //             Ok(WebsocketEvent::Message(msg)) => {
    //                 if let Some(bytes) = msg.bytes() {
    //                     dest_write.send(Message::Binary(bytes)).await;
    //                 }
    //             }
    //             Ok(WebsocketEvent::Close(close)) => {
    //                 console_log!("Close event: {:?}", close);
    //                 error = true;
    //             }
    //             Err(e) => {
    //                 console_log!("Error: {:?}", e);
    //                 error = true;
    //             }
    //         },
    //         Either::Right((Some(dest_event), _)) => match dest_event {
    //             Ok(msg) => {
    //                 let bytes = msg.into_data();
    //                     source_ws_server.send_with_bytes(bytes);
    //             }
    //             Err(e) => {
    //                 console_log!("Error: {:?}", e);
    //                 error = true;
    //             }
    //         },
    //         _ => {
    //             console_log!("No event, error");
    //             error = true;
    //         }
    //     }
    // }
    });

// TODO add better response headers here
    return Response::from_websocket(source_ws.client);

    console_log!("checkin' events");

    // let mut source_events = source_ws.server.events()?;
    // let mut dest_events = dest_ws.events()?;

    console_log!("made events");

    // dest_ws.accept()?;
    console_log!("dest accepted");
    // source_ws.client.accept()?;
    // console_log!("source client accepted");

    // let mut error = false;

    

    plaintext_error("failed to proxy websocket", 503)
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

    if req.path() == "/oliver" {
        return linkup_ws_handler(req, sessions).await;
    }

    if req.method() == Method::Post && req.path() == "/linkup" {
        return linkup_session_handler(req, sessions).await;
    }

    linkup_request_handler(req, sessions).await
}
