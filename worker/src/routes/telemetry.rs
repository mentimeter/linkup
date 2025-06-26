use axum::{
    body::Body,
    extract::{Path, Request, State},
    response::IntoResponse,
    routing::any,
    Router,
};
use http::{StatusCode, Uri};
use worker::{Fetch, HttpResponse};

use crate::{http_error::HttpError, LinkupState};

pub fn router() -> Router<LinkupState> {
    Router::new().route("/linkup/telemetry/otel/{*path}", any(otel_handler))
}

#[worker::send]
async fn otel_handler(
    State(state): State<LinkupState>,
    Path(path): Path<String>,
    mut req: Request,
) -> impl IntoResponse {
    let otlp_config = if let Some(otlp_config) = state.otlp {
        otlp_config
    } else {
        return (StatusCode::NO_CONTENT, Body::empty()).into_response();
    };

    let url = match otlp_config.endpoint.join(&path) {
        Ok(url) => url,
        Err(_) => {
            return HttpError::new("Invalid OTLP URL".into(), StatusCode::INTERNAL_SERVER_ERROR)
                .into_response();
        }
    };

    let uri_string = url.to_string();
    let uri: Uri = match uri_string.parse() {
        Ok(uri) => uri,
        Err(_) => {
            return HttpError::new(
                format!("Invalid URI: {}", uri_string),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    *req.uri_mut() = uri;
    req.headers_mut().extend(otlp_config.headers);

    let worker_request: worker::Request = match req.try_into() {
        Ok(req) => req,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse request: {}", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response()
        }
    };

    let upstream_response = match Fetch::Request(worker_request).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to fetch from target service: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    let resp: HttpResponse = match upstream_response.try_into() {
        Ok(resp) => resp,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse response: {}", e),
                StatusCode::BAD_GATEWAY,
            )
            .into_response()
        }
    };

    resp.into_response()
}
