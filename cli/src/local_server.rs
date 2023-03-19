
use std::{io, sync::Arc, collections::HashMap};

use thiserror::Error;
use actix_web::{Responder, HttpServer, App, web, HttpResponse, middleware, HttpRequest};
use serde_yaml::from_str;
// use bytes::Bytes;

use serpress::*;

use crate::SERPRESS_PORT;


#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("reqwest proxy error {0}")]
    ReqwestProxyError(String),
}

async fn serpress_config_handler(
  session_store: web::Data<Arc<MemorySessionStore>>,
  req_body: web::Bytes,
) -> impl Responder {

  let input_yaml_conf = match String::from_utf8(req_body.to_vec()) {
      Ok(input_yaml_conf) => input_yaml_conf,
      Err(_) => return HttpResponse::BadRequest().body("Invalid request body encoding"),
  };

  match new_server_config_post(input_yaml_conf) {
      Ok((desired_name, server_conf)) => {
          let session_name = session_store.new(server_conf, NameKind::Animal, Some(desired_name));
          HttpResponse::Ok().body(session_name)
      }
      Err(e) => HttpResponse::BadRequest().body(format!("Failed to parse server config: {}", e)),
  }
}


async fn serpress_request_handler(
  session_store: web::Data<MemorySessionStore>,
  req: HttpRequest,
  req_body: web::Bytes,
) -> impl Responder {
  let url = req.uri().to_string();
  let headers = req
      .headers()
      .iter()
      .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
      .collect::<HashMap<String, String>>();

  // TODO Fix
  let result = get_request_session(url, headers.clone(), *session_store);

  match result {
      Ok((session_name, config)) => {
          if let Some((destination_url, service)) = get_target_url(url.clone(), headers.clone(), &config, &session_name) {
              let extra_headers = get_additional_headers(url, headers, &session_name, &service);

              // Proxy the request using the destination_url and the merged headers
              let client = reqwest::Client::new();
              let response = client.request(req.method().clone(), &destination_url)
                  .headers(merge_headers(headers, extra_headers))
                  .body(req_body)
                  .send()
                  .await;

              match response {
                  Ok(response) => convert_reqwest_response(response).await.unwrap_or_else(|_| HttpResponse::InternalServerError().finish()),
                  Err(_) => HttpResponse::BadGateway().finish(),
              }
          } else {
              HttpResponse::NotFound().body("Fallback handler")
          }
      }
      Err(_) => HttpResponse::UnprocessableEntity().body("Unprocessable Content"),
  }
}

fn merge_headers(
  original_headers: HashMap<String, String>,
  extra_headers: HashMap<String, String>,
) -> reqwest::header::HeaderMap {
  let mut header_map = reqwest::header::HeaderMap::new();
  for (key, value) in original_headers.into_iter().chain(extra_headers.into_iter()) {
      if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
          if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&value) {
              header_map.append(header_name, header_value);
          }
      }
  }
  header_map
}

async fn convert_reqwest_response(response: reqwest::Response) -> Result<HttpResponse, ProxyError> {
  let status_code = response.status();
  let headers = response.headers().clone();
  let body = response.bytes().await.map_err(|e| ProxyError::ReqwestProxyError(e.to_string()))?;

  let mut response_builder = HttpResponse::build(status_code);
  for (key, value) in headers.iter() {
      response_builder.append_header((key, value));
  }

  Ok(response_builder.body(body))
}

#[actix_web::main]
pub async fn local_serpress_main() -> io::Result<()> {
  let session_store = web::Data::new(MemorySessionStore::new());

  HttpServer::new(move || {
      App::new()
          .app_data(session_store.clone()) // Add shared state
          .wrap(middleware::Logger::default()) // Enable logger
          .route("/serpress", web::post().to(serpress_config_handler))
          .default_service(web::route().to(serpress_request_handler))
  })
  .bind(("127.0.0.1", SERPRESS_PORT))?
  .run()
  .await
}