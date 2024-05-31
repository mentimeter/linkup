use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct HttpError {
    message: String,
    status_code: StatusCode,
}

impl HttpError {
    pub fn new(message: String, status_code: StatusCode) -> Self {
        HttpError {
            message,
            status_code,
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        Response::builder()
            .status(self.status_code)
            .header("Content-Type", "text/plain")
            .body(axum::body::Body::from(self.message))
            .unwrap()
    }
}
