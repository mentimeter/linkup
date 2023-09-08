use std::collections::HashMap;

use linkup::*;
use worker::*;

use futures::{
    future::{self, Either},
    stream::StreamExt,
};

use crate::http_util::plaintext_error;

pub async fn linkup_ws_handler(req: Request, sessions: SessionAllocator) -> Result<Response> {
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

    let (dest_service_name, destination_url) =
        match get_target_service(url.clone(), headers.clone(), &config, &session_name) {
            Some(result) => result,
            None => return plaintext_error("No target URL for request", 422),
        };

    let extra_headers = get_additional_headers(url, &headers, &session_name, &dest_service_name);
    headers.extend(extra_headers);

    let dest_ws_res = websocket_connect(&destination_url, headers).await;
    let dest_ws = match dest_ws_res {
        Ok(ws) => ws,
        Err(e) => {
            console_log!("Failed to connect to destination: {}", e);
            return plaintext_error(format!("Failed to connect to destination: {}", e), 502);
        }
    };

    let source_ws = WebSocketPair::new()?;
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

    Response::from_websocket(source_ws.client)
}

fn forward_ws_event(
    event: Result<WebsocketEvent>,
    from: &WebSocket,
    to: &WebSocket,
    description: String,
) -> Result<()> {
    match event {
        Ok(WebsocketEvent::Message(msg)) => {
            if let Some(text) = msg.text() {
                match to.send_with_str(text) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let err_msg = format!("Error sending {} with string: {:?}", description, e);
                        close_with_internal_error(err_msg.clone(), &from, &to);
                        Err(Error::RustError(err_msg))
                    }
                }
            } else if let Some(bytes) = msg.bytes() {
                match to.send_with_bytes(bytes) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let err_msg = format!("Error sending {} with bytes: {:?}", description, e);
                        close_with_internal_error(err_msg.clone(), &from, &to);
                        Err(Error::RustError(err_msg))
                    }
                }
            } else {
                let err_msg = format!("Error message {} no text or bytes", description);
                close_with_internal_error(err_msg.clone(), &from, &to);
                Err(Error::RustError(err_msg))
            }
        }
        Ok(WebsocketEvent::Close(close)) => {
            console_log!("Close event from source: {:?}", close);
            let _ = to.close(Some(close.code()), Some(close.reason()));
            Err(Error::RustError(format!("Close event: {}", close.reason())))
        }
        Err(e) => {
            let err_msg = format!("Other {} error: {:?}", description, e);
            close_with_internal_error(err_msg.clone(), &from, &to);
            Err(Error::RustError(err_msg))
        }
    }
}

async fn websocket_connect(
    url: &str,
    additional_headers: HashMap<String, String>,
) -> Result<WebSocket> {
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

    let mut headers = Headers::new();
    additional_headers.iter().for_each(|(k, v)| {
        headers
            .append(k, v)
            .expect("could not append header to websocket request");
    });
    headers.set("upgrade", "websocket")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.with_headers(headers);

    let req = Request::new_with_init(proper_url.as_str(), &init)?;

    let res = Fetch::Request(req).send().await?;

    match res.websocket() {
        Some(ws) => Ok(ws),
        None => Err(Error::RustError("server did not accept".into())),
    }
}

fn close_with_internal_error(msg: String, from: &WebSocket, to: &WebSocket) {
    console_log!("{}", msg);
    let close_res = to.close(Some(1011), Some(msg.clone()));
    if let Err(e) = close_res {
        console_log!("Error closing to websocket: {:?}", e);
    }
    let close_res_dest = from.close(Some(1011), Some(msg));
    if let Err(e) = close_res_dest {
        console_log!("Error closing from websocket: {:?}", e);
    }
}
