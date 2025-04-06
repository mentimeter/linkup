mod local;
mod worker;

#[cfg(feature = "local")]
pub use local::serve_http;

use axum::{Router, response::IntoResponse, routing::get};

fn router() -> Router {
    Router::new().route("/linkup/check", get(handler_check))
}

async fn handler_check() -> impl IntoResponse {
    "Ok!"
}
