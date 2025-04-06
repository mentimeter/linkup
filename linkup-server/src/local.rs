#![cfg(feature = "local")]

use std::net::SocketAddr;

use crate::router;

pub async fn serve_http() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    axum_server::bind(addr)
        .serve(router().into_make_service())
        .await
        .unwrap()
}
