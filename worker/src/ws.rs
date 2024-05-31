use worker::*;

use futures::{
    future::{self, Either},
    stream::StreamExt,
};

pub fn forward_ws_event(
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
            console_log!("Close event from source: {:?}", close);
            let close_res = to.close(Some(close.code()), Some(close.reason()));
            match close_res {
                Err(e) => {
                    console_log!("Error closing {} with close event: {:?}", description, e);
                }
                _ => {}
            };
            Err(Error::RustError(format!("Close event: {}", close.reason())))
        }
        Err(e) => {
            let err_msg = format!("Other {} error: {:?}", description, e);
            close_with_internal_error(err_msg.clone(), from, to);
            Err(Error::RustError(err_msg))
        }
    }
}

pub fn close_with_internal_error(msg: String, from: &WebSocket, to: &WebSocket) {
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
