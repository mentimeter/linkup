
use std::{io, collections::HashMap};

use thiserror::Error;
use actix_web::{Responder, HttpServer, App, web, HttpResponse, middleware, HttpRequest};

use linkup::*;

use crate::LINKUP_PORT;


#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("reqwest proxy error {0}")]
    ReqwestProxyError(String),
}

async fn linkup_config_handler(
  session_store: web::Data<MemorySessionStore>,
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


async fn linkup_request_handler(
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

  let session_result = get_request_session(url.clone(), headers.clone(), |n| session_store.get(n));

  let (session_name, config) = match session_result {
      Ok(result) => result,
      Err(_) => return HttpResponse::UnprocessableEntity().body("Unprocessable Content"),
  };

  let (destination_url, service) = match get_target_url(url.clone(), headers.clone(), &config, &session_name) {
      Some(result) => result,
      None => return HttpResponse::NotFound().body("Fallback handler"),
  };

  let extra_headers = get_additional_headers(url, &headers, &session_name, &service);

  // Proxy the request using the destination_url and the merged headers
  let client = reqwest::Client::new();
  let response_result = client
      .request(req.method().clone(), &destination_url)
      .headers(merge_headers(headers, extra_headers))
      .body(req_body)
      .send()
      .await;

  let response = match response_result {
      Ok(response) => response,
      Err(_) => return HttpResponse::BadGateway().finish(),
  };

  convert_reqwest_response(response).await.unwrap_or_else(|_| HttpResponse::InternalServerError().finish())
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
pub async fn local_linkup_main() -> io::Result<()> {
  let session_store = web::Data::new(MemorySessionStore::new());

  HttpServer::new(move || {
      App::new()
          .app_data(session_store.clone()) // Add shared state
          .wrap(middleware::Logger::default()) // Enable logger
          .route("/linkup", web::post().to(linkup_config_handler))
          .default_service(web::route().to(linkup_request_handler))
  })
  .bind(("127.0.0.1", LINKUP_PORT))?
  .run()
  .await
}