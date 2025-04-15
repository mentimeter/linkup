use std::str::FromStr;

use axum::{http::StatusCode, response::IntoResponse};
use http::{HeaderMap, HeaderName, HeaderValue};
use linkup::allow_all_cors;
use worker::{console_log, Error, HttpResponse, WebSocket, WebSocketPair, WebsocketEvent};

use futures::{
    future::{self, Either},
    stream::StreamExt,
};

use crate::http_error::HttpError;

pub async fn handle_ws_resp(upstream_response: worker::Response) -> impl IntoResponse {
    let upstream_response_headers = upstream_response.headers().clone();
    let upstream_ws_result = match upstream_response.websocket() {
        Some(ws) => Ok(ws),
        None => Err(Error::RustError("server did not accept".into())),
    };
    let upstream_ws = match upstream_ws_result {
        Ok(ws) => ws,
        Err(e) => {
            return HttpError::new(
                format!("Failed to connect to destination: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let downstream_ws = match WebSocketPair::new() {
        Ok(ws) => ws,
        Err(e) => {
            return HttpError::new(
                format!("Failed to create source websocket: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };
    let downstream_ws_server = downstream_ws.server;

    worker::wasm_bindgen_futures::spawn_local(async move {
        let mut upstream_events = upstream_ws
            .events()
            .expect("could not open dest event stream");
        let mut downstream_events = downstream_ws_server
            .events()
            .expect("could not open source event stream");

        upstream_ws.accept().expect("could not accept dest ws");
        downstream_ws_server
            .accept()
            .expect("could not accept source ws");

        let mut is_closed = false;

        loop {
            match future::select(downstream_events.next(), upstream_events.next()).await {
                Either::Left((Some(downstream_event), _)) => {
                    if let Err(e) = forward_ws_event(
                        downstream_event,
                        &downstream_ws_server,
                        &upstream_ws,
                        "to destination".into(),
                        &mut is_closed,
                    ) {
                        console_log!("Error forwarding source event: {:?}", e);
                        break;
                    }
                }
                Either::Right((Some(upstream_event), _)) => {
                    if let Err(e) = forward_ws_event(
                        upstream_event,
                        &upstream_ws,
                        &downstream_ws_server,
                        "to source".into(),
                        &mut is_closed,
                    ) {
                        console_log!("Error forwarding dest event: {:?}", e);
                        break;
                    }
                }
                _ => {
                    console_log!("No event received, error");
                    close_with_internal_error(
                        "Received something other than event from streams".to_string(),
                        &downstream_ws_server,
                        &upstream_ws,
                    );
                    break;
                }
            }
        }
    });

    let downstream_resp = match worker::Response::from_websocket(downstream_ws.client) {
        Ok(res) => res,
        Err(e) => {
            return HttpError::new(
                format!("Failed to create response from websocket: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };

    let mut resp: HttpResponse = match downstream_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    for upstream_header in upstream_response_headers.entries() {
        if !resp.headers().contains_key(&upstream_header.0) {
            resp.headers_mut().append(
                HeaderName::from_str(&upstream_header.0).unwrap(),
                HeaderValue::from_str(&upstream_header.1).unwrap(),
            );
        }
    }

    resp.headers_mut().extend(allow_all_cors());

    resp.into_response()
}

fn forward_ws_event(
    event: Result<WebsocketEvent, Error>,
    from: &WebSocket,
    to: &WebSocket,
    description: String,
    is_closed: &mut bool,
) -> Result<(), Error> {
    match event {
        Ok(WebsocketEvent::Message(msg)) => {
            if let Some(text) = msg.text() {
                match to.send_with_str(text) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let err_msg = format!("Error sending {} with string: {:?}", description, e);
                        close_with_internal_error(err_msg.clone(), from, to);
                        Err(Error::RustError(err_msg))
                    }
                }
            } else if let Some(bytes) = msg.bytes() {
                match to.send_with_bytes(bytes) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let err_msg = format!("Error sending {} with bytes: {:?}", description, e);
                        close_with_internal_error(err_msg.clone(), from, to);
                        Err(Error::RustError(err_msg))
                    }
                }
            } else {
                let err_msg = format!("Error message {} no text or bytes", description);
                close_with_internal_error(err_msg.clone(), from, to);
                Err(Error::RustError(err_msg))
            }
        }
        Ok(WebsocketEvent::Close(close)) => {
            let _ = to.close(Some(1000), Some(close.reason()));
            if *is_closed {
                return Err(Error::RustError("Closed!".into()));
            } else {
                *is_closed = true;
            }

            Ok(())
        }
        Err(e) => {
            let err_msg = format!("Other {} error: {:?}", description, e);
            close_with_internal_error(err_msg.clone(), from, to);
            Err(Error::RustError(err_msg))
        }
    }
}

fn close_with_internal_error(msg: String, from: &WebSocket, to: &WebSocket) {
    console_log!("close message: {}", msg);
    let close_res = to.close(Some(1011), Some(msg.clone()));
    if let Err(e) = close_res {
        console_log!("Error closing to websocket: {:?}", e);
    }
    let close_res_dest = from.close(Some(1011), Some(msg));
    if let Err(e) = close_res_dest {
        console_log!("Error closing from websocket: {:?}", e);
    }
}
