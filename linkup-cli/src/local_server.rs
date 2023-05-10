use std::{collections::HashMap, io};

use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use thiserror::Error;

use linkup::*;

use crate::LINKUP_LOCALSERVER_PORT;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("reqwest proxy error {0}")]
    ReqwestProxyError(String),
}

async fn linkup_config_handler(
    string_store: web::Data<MemoryStringStore>,
    req_body: web::Bytes,
) -> impl Responder {
    let sessions = SessionAllocator::new(string_store.into_inner());

    let input_json_conf = match String::from_utf8(req_body.to_vec()) {
        Ok(input_json_conf) => input_json_conf,
        Err(_) => return HttpResponse::BadRequest().body("Invalid request body encoding"),
    };

    match update_session_req_from_json(input_json_conf) {
        Ok((desired_name, server_conf)) => {
            let session_name = sessions
                .store_session(server_conf, NameKind::Animal, desired_name)
                .await;
            match session_name {
                Ok(session_name) => HttpResponse::Ok().body(session_name),
                Err(e) => HttpResponse::InternalServerError()
                    .body(format!("Failed to store server config: {}", e)),
            }
        }
        Err(e) => HttpResponse::BadRequest().body(format!("Failed to parse server config: {}", e)),
    }
}

async fn linkup_request_handler(
    string_store: web::Data<MemoryStringStore>,
    req: HttpRequest,
    req_body: web::Bytes,
) -> impl Responder {
    let sessions = SessionAllocator::new(string_store.into_inner());

    let url = format!("http://localhost:9066{}", req.uri());
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect::<HashMap<String, String>>();

    let session_result = sessions
        .get_request_session(url.clone(), headers.clone())
        .await;

    let (session_name, config) = match session_result {
        Ok(result) => result,
        Err(_) => return HttpResponse::UnprocessableEntity().body("Unprocessable Content"),
    };

    let destination_url  =
        match get_target_url(url.clone(), headers.clone(), &config, &session_name) {
            Some(result) => result,
            None => return HttpResponse::NotFound().body("Not target url for request"),
        };

    let extra_headers = get_additional_headers(url, &headers, &session_name);

    // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::new();
    let response_result = client
        .request(req.method().clone(), &destination_url)
        .headers(merge_headers(&headers, &extra_headers))
        .body(req_body)
        .send()
        .await;

    let response = match response_result {
        Ok(response) => response,
        Err(_) => return HttpResponse::BadGateway().finish(),
    };

    convert_reqwest_response(response)
        .await
        .unwrap_or_else(|_| HttpResponse::InternalServerError().finish())
}

fn merge_headers(
    original_headers: &HashMap<String, String>,
    extra_headers: &HashMap<String, String>,
) -> reqwest::header::HeaderMap {
    let mut header_map = reqwest::header::HeaderMap::new();
    for (key, value) in original_headers.iter().chain(extra_headers.iter()) {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                header_map.append(header_name, header_value);
            }
        }
    }
    header_map
}

async fn convert_reqwest_response(response: reqwest::Response) -> Result<HttpResponse, ProxyError> {
    let status_code = response.status();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(|e| ProxyError::ReqwestProxyError(e.to_string()))?;

    let mut response_builder = HttpResponse::build(status_code);
    for (key, value) in headers.iter() {
        response_builder.append_header((key, value));
    }

    Ok(response_builder.body(body))
}

#[actix_web::main]
pub async fn local_linkup_main() -> io::Result<()> {
    env_logger::Builder::new()
        .filter(None, log::LevelFilter::Info)
        .init();

    let string_store = web::Data::new(MemoryStringStore::new());

    println!("Starting local server on port {}", LINKUP_LOCALSERVER_PORT);
    HttpServer::new(move || {
        App::new()
            .app_data(string_store.clone()) // Add shared state
            .wrap(middleware::Logger::default()) // Enable logger
            .route("/linkup", web::post().to(linkup_config_handler))
            .default_service(web::route().to(linkup_request_handler))
    })
    .bind(("127.0.0.1", LINKUP_LOCALSERVER_PORT))?
    .run()
    .await
}
