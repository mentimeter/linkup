pub mod proxy;
pub mod sessions;

use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use http::StatusCode;

#[derive(Debug)]
pub struct ApiError {
    message: String,
    status_code: StatusCode,
}

impl ApiError {
    pub fn new(message: String, status_code: StatusCode) -> Self {
        ApiError {
            message,
            status_code,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status_code)
            .header("Content-Type", "text/plain")
            .body(Body::from(self.message))
            .unwrap()
    }
}

pub async fn always_ok() -> &'static str {
    "OK"
}
