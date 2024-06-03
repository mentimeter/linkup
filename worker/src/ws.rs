use axum::{http::StatusCode, response::IntoResponse};
use linkup::allow_all_cors;
use worker::{console_log, Error, HttpResponse, WebSocket, WebSocketPair, WebsocketEvent};

use futures::{
    future::{self, Either},
    stream::StreamExt,
};

use crate::http_error::HttpError;

pub async fn handle_ws_resp(worker_resp: worker::Response) -> impl IntoResponse {
    let dest_ws_res = match worker_resp.websocket() {
        Some(ws) => Ok(ws),
        None => Err(Error::RustError("server did not accept".into())),
    };
    let dest_ws = match dest_ws_res {
        Ok(ws) => ws,
        Err(e) => {
            return HttpError::new(
                format!("Failed to connect to destination: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let source_ws = match WebSocketPair::new() {
        Ok(ws) => ws,
        Err(e) => {
            return HttpError::new(
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
            return HttpError::new(
                format!("Failed to create response from websocket: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response()
        }
    };
    let mut resp: HttpResponse = match worker_resp.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    resp.headers_mut().extend(allow_all_cors());

    resp.into_response()
}

fn forward_ws_event(
    event: Result<WebsocketEvent, Error>,
    from: &WebSocket,
    to: &WebSocket,
    description: String,
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
            let close_res = to.close(Some(1000), Some(close.reason()));
            if let Err(e) = close_res {
                console_log!("Error closing {} with close event: {:?}", description, e);
            }
            Err(Error::RustError(format!("Close event: {}", close.reason())))
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
