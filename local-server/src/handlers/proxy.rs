use axum::{
    body::Body,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use linkup::{TargetService, get_additional_headers, get_target_service};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::{HttpsClient, ServerState, handlers::ApiError, ws};

pub async fn handle_all(
    State(server_state): State<ServerState>,
    ws: ws::ExtractOptionalWebSocketUpgrade,
    req: Request,
) -> Response {
    let headers: linkup::HeaderMap = req.headers().into();
    let url = if req.uri().scheme().is_some() {
        req.uri().to_string()
    } else {
        format!(
            "http://{}{}",
            req.headers()
                .get(http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("localhost"),
            req.uri()
        )
    };

    let (session_name, config) = match server_state.session_allocator.get_request_session(&url, &headers).await {
        Ok(session) => session,
        Err(_) => {
            return ApiError::new(
                "Linkup was unable to determine the session origin of the request. Ensure that your request includes a valid session identifier in the referer or tracestate headers. - Local Server".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
            )
                .into_response()
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return ApiError::new(
                "The request belonged to a session, but there was no target for the request. Check that the routing rules in your linkup config have a match for this request. - Local Server".to_string(),
                StatusCode::NOT_FOUND,
            )
                .into_response()
        }
    };

    let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);

    match ws.0 {
        Some(downstream_upgrade) => {
            let mut url = target_service.url;
            if url.starts_with("http://") {
                url = url.replace("http://", "ws://");
            } else if url.starts_with("https://") {
                url = url.replace("https://", "wss://");
            }

            let uri = url.parse::<Uri>().unwrap();
            let host = uri.host().unwrap().to_string();
            let mut upstream_request = uri.into_client_request().unwrap();

            // Copy over all headers from the incoming request
            let mut cookie_values: Vec<String> = Vec::new();
            for (key, value) in req.headers() {
                if key == http::header::COOKIE {
                    if let Ok(cookie_value) = value.to_str().map(str::trim)
                        && !cookie_value.is_empty()
                    {
                        cookie_values.push(cookie_value.to_string());
                    }

                    continue;
                }

                upstream_request.headers_mut().insert(key, value.clone());
            }

            if !cookie_values.is_empty() {
                let combined = cookie_values.join("; ");
                if let Ok(cookie_header_value) = HeaderValue::from_str(&combined) {
                    upstream_request
                        .headers_mut()
                        .insert(http::header::COOKIE, cookie_header_value);
                }
            }

            linkup::normalize_cookie_header(upstream_request.headers_mut());

            // add the extra headers that linkup wants
            let extra_http_headers: HeaderMap = extra_headers.into();
            for (key, value) in extra_http_headers.iter() {
                upstream_request.headers_mut().insert(key, value.clone());
            }

            // Overriding host header neccesary for tokio_tungstenite
            upstream_request
                .headers_mut()
                .insert(http::header::HOST, HeaderValue::from_str(&host).unwrap());

            let (upstream_ws_stream, upstream_response) =
                match tokio_tungstenite::connect_async(upstream_request).await {
                    Ok(connection) => connection,
                    Err(error) => match error {
                        tokio_tungstenite::tungstenite::Error::Http(response) => {
                            let (parts, body) = response.into_parts();
                            let body = body.unwrap_or_default();

                            return Response::from_parts(parts, Body::from(body));
                        }
                        error => {
                            return Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .body(Body::from(error.to_string()))
                                .unwrap();
                        }
                    },
                };

            let mut downstream_upgrade_response =
                downstream_upgrade.on_upgrade(ws::context_handle_socket(upstream_ws_stream));

            let downstream_response_headers = downstream_upgrade_response.headers_mut();

            // The headers from the upstream response are more important - trust the upstream server
            for (upstream_key, upstream_value) in upstream_response.headers() {
                // Except for content encoding headers, cloudflare does _not_ like them..
                if !DISALLOWED_HEADERS.contains(upstream_key) {
                    downstream_response_headers
                        .insert(upstream_key.clone(), upstream_value.clone());
                }
            }

            downstream_response_headers.extend(linkup::allow_all_cors());

            downstream_upgrade_response
        }
        None => {
            handle_http_req(
                req,
                target_service,
                extra_headers,
                server_state.https_client,
            )
            .await
        }
    }
}

const DISALLOWED_HEADERS: [HeaderName; 2] = [
    HeaderName::from_static("content-encoding"),
    HeaderName::from_static("content-length"),
];

async fn handle_http_req(
    mut req: Request,
    target_service: TargetService,
    extra_headers: linkup::HeaderMap,
    client: HttpsClient,
) -> Response {
    *req.uri_mut() = Uri::try_from(&target_service.url).unwrap();
    let extra_http_headers: HeaderMap = extra_headers.into();
    req.headers_mut().extend(extra_http_headers);
    // Request uri and host headers should not conflict
    req.headers_mut().remove(http::header::HOST);
    linkup::normalize_cookie_header(req.headers_mut());

    if target_service.url.starts_with("http://") {
        *req.version_mut() = http::Version::HTTP_11;
    }

    // Send the modified request to the target service.
    let mut resp = match client.request(req).await {
        Ok(resp) => resp,
        Err(e) => {
            return ApiError::new(
                format!(
                    "Failed to proxy request - are all your servers started? {}",
                    e
                ),
                StatusCode::BAD_GATEWAY,
            )
            .into_response();
        }
    };

    resp.headers_mut().extend(linkup::allow_all_cors());

    resp.into_response()
}
