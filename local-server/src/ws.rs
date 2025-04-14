use std::{future::Future, pin::Pin};

use axum::extract::{ws::WebSocket, FromRequestParts, WebSocketUpgrade};
use http::{request::Parts, StatusCode};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

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

pub fn context_handle_socket(
    _upstream_ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Box<dyn FnOnce(WebSocket) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send> {
    Box::new(move |mut socket: WebSocket| {
        Box::pin(async move {
            while let Some(msg_result) = socket.recv().await {
                let msg = match msg_result {
                    Ok(msg) => msg,
                    Err(_) => return,
                };

                if socket.send(msg).await.is_err() {
                    return;
                }
            }
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
