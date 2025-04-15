use std::{future::Future, pin::Pin};

use axum::extract::{ws::WebSocket, FromRequestParts, WebSocketUpgrade};
use futures::{SinkExt, StreamExt};
use http::{request::Parts, StatusCode};
use tokio::{net::TcpStream, select};
use tokio_tungstenite::{
    tungstenite::{self, Message},
    MaybeTlsStream, WebSocketStream,
};

pub struct ExtractOptionalWebSocketUpgrade(pub Option<WebSocketUpgrade>);

impl<S> FromRequestParts<S> for ExtractOptionalWebSocketUpgrade
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let upgrade = WebSocketUpgrade::from_request_parts(parts, state).await;

        match upgrade {
            Ok(upgrade) => Ok(ExtractOptionalWebSocketUpgrade(Some(upgrade))),
            Err(_) => {
                // TODO: Maybe log?
                Ok(ExtractOptionalWebSocketUpgrade(None))
            }
        }
    }
}

pub fn tungstenite_to_axum(message: tungstenite::Message) -> axum::extract::ws::Message {
    match message {
        Message::Text(utf8_bytes) => axum::extract::ws::Message::Text(
            axum::extract::ws::Utf8Bytes::from(utf8_bytes.as_str()),
        ),
        Message::Binary(bytes) => axum::extract::ws::Message::Binary(bytes),
        Message::Ping(bytes) => axum::extract::ws::Message::Ping(bytes),
        Message::Pong(bytes) => axum::extract::ws::Message::Pong(bytes),
        Message::Close(close_frame) => match close_frame {
            Some(frame) => axum::extract::ws::Message::Close(Some(axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: axum::extract::ws::Utf8Bytes::from(frame.reason.as_str()),
            })),
            None => axum::extract::ws::Message::Close(None),
        },
        Message::Frame(_frame) => unreachable!(),
    }
}

pub fn axum_to_tungstenite(message: axum::extract::ws::Message) -> tungstenite::Message {
    match message {
        axum::extract::ws::Message::Text(utf8_bytes) => {
            tungstenite::Message::Text(tungstenite::Utf8Bytes::from(utf8_bytes.as_str()))
        }
        axum::extract::ws::Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
        axum::extract::ws::Message::Ping(bytes) => tungstenite::Message::Ping(bytes),
        axum::extract::ws::Message::Pong(bytes) => tungstenite::Message::Pong(bytes),
        axum::extract::ws::Message::Close(close_frame) => {
            tungstenite::Message::Close(close_frame.map(|frame| {
                tungstenite::protocol::frame::CloseFrame {
                    code: frame.code.into(),
                    reason: tungstenite::Utf8Bytes::from(frame.reason.as_str()),
                }
            }))
        }
    }
}

type WrappedSocketHandler =
    Box<dyn FnOnce(WebSocket) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

pub fn context_handle_socket(
    upstream_ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> WrappedSocketHandler {
    Box::new(move |downstream: WebSocket| {
        Box::pin(async move {
            let (mut upstream_write, mut upstream_read) = upstream_ws.split();
            let (mut downstream_write, mut downstream_read) = downstream.split();

            let mut is_closed = false;

            loop {
                select! {
                    Some(message) = upstream_read.next() => {
                        match message {
                            Ok(message) => {
                                let axum_message = tungstenite_to_axum(message);

                                match &axum_message {
                                    axum::extract::ws::Message::Close(_) => {
                                        let _ = downstream_write.send(axum_message).await;

                                        if is_closed {
                                            break;
                                        } else {
                                            is_closed = true;
                                        }
                                    },
                                    _ => {
                                        if let Err(e) = downstream_write.send(axum_message).await {
                                            eprintln!("Error sending message to upstream: {}", e);
                                            break;
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                eprint!("Got error on reading message from upstream: {}", e);
                                break;
                            },
                        }
                    }
                    Some(message) = downstream_read.next() => {
                        match message {
                            Ok(message) => {
                                let tungstenite_message = axum_to_tungstenite(message);

                                match &tungstenite_message {
                                    Message::Close(_) => {
                                        let _ = upstream_write.send(tungstenite_message).await;

                                        if is_closed {
                                            break;
                                        } else {
                                            is_closed = true;
                                        }
                                    },
                                    _ => {
                                        if let Err(e) = upstream_write.send(tungstenite_message).await {
                                            eprintln!("Error sending message to upstream: {}", e);
                                            break;
                                        }
                                    }
                                }
                            },
                            Err(e) => {
                                eprint!("Got error on reading message from downstream: {}", e);
                                break;
                            },
                        }
                    }
                }
            }

            let _ = upstream_write.close().await;
            let _ = downstream_write.close().await;
        })
    })
}

// // Old websocket handler
//
// async fn handle_ws_req(
//     req: Request,
//     target_service: TargetService,
//     extra_headers: linkup::HeaderMap,
//     client: HttpsClient,
// ) -> Response {
//     let extra_http_headers: HeaderMap = extra_headers.into();

//     let target_ws_req_result = Request::builder()
//         .uri(target_service.url)
//         .method(req.method().clone())
//         .body(Body::empty());

//     let mut target_ws_req = match target_ws_req_result {
//         Ok(request) => request,
//         Err(e) => {
//             return ApiError::new(
//                 format!("Failed to build request: {}", e),
//                 StatusCode::INTERNAL_SERVER_ERROR,
//             )
//             .into_response();
//         }
//     };

//     target_ws_req.headers_mut().extend(req.headers().clone());
//     target_ws_req.headers_mut().extend(extra_http_headers);
//     target_ws_req.headers_mut().remove(http::header::HOST);

//     // Send the modified request to the target service.
//     let target_ws_resp = match client.request(target_ws_req).await {
//         Ok(resp) => resp,
//         Err(e) => {
//             return ApiError::new(
//                 format!("Failed to proxy request: {}", e),
//                 StatusCode::BAD_GATEWAY,
//             )
//             .into_response()
//         }
//     };

//     let status = target_ws_resp.status();
//     if status != 101 {
//         return ApiError::new(
//             format!(
//                 "Failed to proxy request: expected 101 Switching Protocols, got {}",
//                 status
//             ),
//             StatusCode::BAD_GATEWAY,
//         )
//         .into_response();
//     }

//     let target_ws_resp_headers = target_ws_resp.headers().clone();

//     let upgraded_target = match hyper::upgrade::on(target_ws_resp).await {
//         Ok(upgraded) => upgraded,
//         Err(e) => {
//             return ApiError::new(
//                 format!("Failed to upgrade connection: {}", e),
//                 StatusCode::BAD_GATEWAY,
//             )
//             .into_response()
//         }
//     };

//     tokio::spawn(async move {
//         // We won't get passed this until the 101 response returns to the client
//         let upgraded_incoming = match hyper::upgrade::on(req).await {
//             Ok(upgraded) => upgraded,
//             Err(e) => {
//                 println!("Failed to upgrade incoming connection: {}", e);
//                 return;
//             }
//         };

//         let mut incoming_stream = TokioIo::new(upgraded_incoming);
//         let mut target_stream = TokioIo::new(upgraded_target);

//         let res = tokio::io::copy_bidirectional(&mut incoming_stream, &mut target_stream).await;

//         match res {
//             Ok((incoming_to_target, target_to_incoming)) => {
//                 println!(
//                     "Copied {} bytes from incoming to target and {} bytes from target to incoming",
//                     incoming_to_target, target_to_incoming
//                 );
//             }
//             Err(e) => {
//                 eprintln!("Error copying between incoming and target: {}", e);
//             }
//         }
//     });

//     let mut resp_builder = Response::builder().status(101);
//     let resp_headers_result = resp_builder.headers_mut();
//     if let Some(resp_headers) = resp_headers_result {
//         for (header, value) in target_ws_resp_headers {
//             if let Some(header_name) = header {
//                 resp_headers.append(header_name, value);
//             }
//         }
//     }

//     match resp_builder.body(Body::empty()) {
//         Ok(response) => response,
//         Err(e) => ApiError::new(
//             format!("Failed to build response: {}", e),
//             StatusCode::INTERNAL_SERVER_ERROR,
//         )
//         .into_response(),
//     }
// }
