use std::str::FromStr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::Router;
use futures::{SinkExt, StreamExt};
use helpers::ServerKind;
use http::Uri;
use rstest::rstest;
use tokio::net::TcpListener;

use crate::helpers::{create_session_request, post, setup_server};

mod helpers;

#[rstest]
#[tokio::test]
async fn can_request_underlying_websocket_server(
    #[values(ServerKind::Local, ServerKind::Worker)] server_kind: ServerKind,
) {
    let url = setup_server(server_kind).await;
    let ws_url = setup_websocket_server().await;

    let session_req = create_session_request("ws-session".to_string(), Some(ws_url));
    let session_resp = post(format!("{}/linkup", url), session_req).await;
    assert_eq!(session_resp.status(), reqwest::StatusCode::OK);
    assert_eq!(session_resp.text().await.unwrap(), "ws-session");

    // Connect to the WebSocket server through the proxy
    let uri = Uri::from_str(url.as_str()).unwrap();
    let req = http::Request::builder()
        .uri(format!("ws://{}/ws", uri.authority().unwrap()))
        .header("referer", "example.com")
        .header("traceparent", "xzyabc")
        .header("tracestate", "linkup-session=ws-session")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .header("sec-websocket-version", "13")
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("host", uri.authority().unwrap().to_string())
        .body(())
        .unwrap();

    let (mut ws_stream, ws_resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("Failed to connect to WebSocket server");

    assert_eq!(ws_resp.status(), 101);

    // Send a message
    let msg = "Hello, WebSocket!";
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
        .await
        .expect("Failed to send message");

    match ws_stream.next().await {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
            assert_eq!(text, msg);
        }
        anythingelse => {
            println!("{:?}", anythingelse);
            panic!("Failed to receive message")
        }
    }

    ws_stream
        .close(None)
        .await
        .expect("Failed to close WebSocket");
}

async fn websocket_echo(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_websocket)
}

async fn handle_websocket(mut socket: WebSocket) {
    println!("WebSocket connected");
    while let Some(result) = socket.recv().await {
        match result {
            Ok(msg) => {
                println!("Received message: {:?}", msg);
                if let Message::Text(text) = msg {
                    if let Err(e) = socket.send(Message::Text(text)).await {
                        println!("Failed to send message: {:?}", e);
                        break;
                    }
                } else if let Message::Close(_) = msg {
                    if let Err(e) = socket.close().await {
                        println!("Failed to close: {:?}", e);
                    }
                    break;
                }
            }
            Err(e) => {
                println!("WebSocket error: {:?}", e);
                break;
            }
        }
    }
    println!("WebSocket disconnected");
}

async fn setup_websocket_server() -> String {
    let app = Router::new().route("/ws", axum::routing::get(websocket_echo));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}", addr)
}
