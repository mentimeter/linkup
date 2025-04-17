use std::{future::Future, pin::Pin};

use axum::extract::{ws::WebSocket, FromRequestParts, WebSocketUpgrade};
use futures::{SinkExt, StreamExt};
use http::{request::Parts, StatusCode};
use tokio::net::TcpStream;
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

fn tungstenite_to_axum(message: tungstenite::Message) -> axum::extract::ws::Message {
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

fn axum_to_tungstenite(message: axum::extract::ws::Message) -> tungstenite::Message {
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
            use futures::future::{select, Either};

            let (mut upstream_write, mut upstream_read) = upstream_ws.split();
            let (mut downstream_write, mut downstream_read) = downstream.split();

            let mut is_closed = false;

            loop {
                match select(downstream_read.next(), upstream_read.next()).await {
                    Either::Left((Some(downstream_message), _)) => match downstream_message {
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
                                }
                                _ => {
                                    if let Err(e) = upstream_write.send(tungstenite_message).await {
                                        eprintln!("Error sending message to upstream: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprint!("Got error on reading message from downstream: {}", e);
                            break;
                        }
                    },
                    Either::Right((Some(upstream_message), _)) => match upstream_message {
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
                                }
                                _ => {
                                    if let Err(e) = downstream_write.send(axum_message).await {
                                        eprintln!("Error sending message to upstream: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprint!("Got error on reading message from upstream: {}", e);
                            break;
                        }
                    },
                    other => {
                        // TODO: On the select! macro, if nothing is matched, it panics. I guess
                        // this might be better than panicking? Or do we want to "fail loudly" here?
                        //
                        // https://docs.rs/tokio/latest/tokio/macro.select.html#panics
                        eprint!("Received unexpected message: {other:?}");

                        break;
                    }
                }
            }

            let _ = upstream_write.close().await;
            let _ = downstream_write.close().await;
        })
    })
}
