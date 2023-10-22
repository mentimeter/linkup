use std::io;

use actix_web::{
    guard, http::header::ContentType, middleware, rt, web, App, HttpRequest, HttpResponse,
    HttpServer, Responder,
};
use futures::stream::StreamExt;
use thiserror::Error;

use linkup::{HeaderMap as LinkupHeaderMap, HeaderName as LinkupHeaderName, *};
use url::Url;

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
    let sessions = SessionAllocator::new(string_store.as_ref());

    let input_json_conf = match String::from_utf8(req_body.to_vec()) {
        Ok(input_json_conf) => input_json_conf,
        Err(_) => {
            return HttpResponse::BadRequest()
                .append_header(ContentType::plaintext())
                .body("Invalid request body encoding - local server")
        }
    };

    match update_session_req_from_json(input_json_conf) {
        Ok((desired_name, server_conf)) => {
            let session_name = sessions
                .store_session(server_conf, NameKind::Animal, desired_name)
                .await;
            match session_name {
                Ok(session_name) => HttpResponse::Ok().body(session_name),
                Err(e) => HttpResponse::InternalServerError()
                    .append_header(ContentType::plaintext())
                    .body(format!("Failed to store server config: {}", e)),
            }
        }
        Err(e) => HttpResponse::BadRequest()
            .append_header(ContentType::plaintext())
            .body(format!(
                "Failed to parse server config: {} - local server",
                e
            )),
    }
}

async fn linkup_ws_request_handler(
    string_store: web::Data<MemoryStringStore>,
    req: HttpRequest,
    req_stream: web::Payload,
) -> impl Responder {
    let sessions = SessionAllocator::new(string_store.as_ref());

    let url = format!("http://localhost:{}{}", LINKUP_LOCALSERVER_PORT, req.uri());
    let mut headers = LinkupHeaderMap::from_actix_request(&req);

    let session_result = sessions.get_request_session(&url, &headers).await;

    if session_result.is_err() {
        println!("Failed to get session: {:?}", session_result);
    }

    let (session_name, config) = match session_result {
        Ok(result) => result,
        Err(_) => {
            return HttpResponse::UnprocessableEntity()
                .append_header(ContentType::plaintext())
                .body("Unprocessable Content - local server")
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return HttpResponse::NotFound()
                .append_header(ContentType::plaintext())
                .body("Not target url for request - local server")
        }
    };

    let extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);

    // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::new();
    headers.extend(&extra_headers);
    let response_result = client
        .request(req.method().clone(), &target_service.url)
        .headers(headers.into())
        .send()
        .await;

    let response =
        match response_result {
            Ok(response) => response,
            Err(_) => return HttpResponse::BadGateway()
                .append_header(ContentType::plaintext())
                .body(
                    "Bad Gateway from local server, could you have forgotten to start the server?",
                ),
        };

    // Make sure the server is willing to accept the websocket.
    let status = response.status().as_u16();
    if status != 101 {
        return HttpResponse::BadGateway()
            .append_header(ContentType::plaintext())
            .body("The underlying server did not accept the websocket connection.");
    }

    // Copy headers from the target back to the client.
    let mut client_response = HttpResponse::SwitchingProtocols();
    client_response.upgrade("websocket");
    for (header, value) in response.headers() {
        client_response.insert_header((header.to_owned(), value.to_owned()));
    }
    for (header, value) in &additional_response_headers() {
        client_response.insert_header((header.to_string(), value.to_string()));
    }

    let upgrade_result = response.upgrade().await;
    let upgrade = match upgrade_result {
        Ok(response) => response,
        Err(_) => {
            return HttpResponse::BadGateway()
                .append_header(ContentType::plaintext())
                .body("could not upgrade to websocket connection.")
        }
    };

    let (target_rx, mut target_tx) = tokio::io::split(upgrade);

    // Copy byte stream from the client to the target.
    rt::spawn(async move {
        let mut req_stream = req_stream.map(|result| {
            result.map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
        });
        let mut client_read = tokio_util::io::StreamReader::new(&mut req_stream);
        let result = tokio::io::copy(&mut client_read, &mut target_tx).await;
        if let Err(err) = result {
            println!("Error proxying websocket client bytes to target: {err}")
        }
    });

    // // Copy byte stream from the target back to the client.
    let target_stream = tokio_util::io::ReaderStream::new(target_rx);
    client_response.streaming(target_stream)
}

async fn linkup_request_handler(
    string_store: web::Data<MemoryStringStore>,
    req: HttpRequest,
    req_body: web::Bytes,
) -> impl Responder {
    let sessions = SessionAllocator::new(string_store.as_ref());

    let url = format!("http://localhost:{}{}", LINKUP_LOCALSERVER_PORT, req.uri());
    let mut headers = LinkupHeaderMap::from_actix_request(&req);

    let session_result = sessions.get_request_session(&url, &headers).await;

    if session_result.is_err() {
        println!("Failed to get session: {:?}", session_result);
    }

    let (session_name, config) = match session_result {
        Ok(result) => result,
        Err(_) => {
            return HttpResponse::UnprocessableEntity()
                .append_header(ContentType::plaintext())
                .body("Unprocessable Content - local server")
        }
    };

    let target_service = match get_target_service(&url, &headers, &config, &session_name) {
        Some(result) => result,
        None => {
            return HttpResponse::NotFound()
                .append_header(ContentType::plaintext())
                .body("Not target url for request - local server")
        }
    };

    let mut extra_headers = get_additional_headers(&url, &headers, &session_name, &target_service);
    extra_headers.insert(
        LinkupHeaderName::Host,
        Url::parse(&target_service.url).unwrap(),
    );

    // Proxy the request using the destination_url and the merged headers
    let client = reqwest::Client::new();
    headers.extend(&extra_headers);

    let response_result = client
        .request(req.method().clone(), &target_service.url)
        .headers(headers.into())
        .body(req_body)
        .send()
        .await;

    let response =
        match response_result {
            Ok(response) => response,
            Err(_) => return HttpResponse::BadGateway()
                .append_header(ContentType::plaintext())
                .body(
                    "Bad Gateway from local server, could you have forgotten to start the server?",
                ),
        };

    convert_reqwest_response(response, &additional_response_headers())
        .await
        .unwrap_or_else(|_| {
            HttpResponse::InternalServerError()
                .append_header(ContentType::plaintext())
                .body("Could not convert response from reqwest - local server")
        })
}

async fn convert_reqwest_response(
    response: reqwest::Response,
    extra_headers: &LinkupHeaderMap,
) -> Result<HttpResponse, ProxyError> {
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

    for (key, value) in extra_headers.into_iter() {
        response_builder.insert_header((key.to_string(), value.to_string()));
    }

    Ok(response_builder.body(body))
}

async fn always_ok() -> impl Responder {
    HttpResponse::Ok().finish()
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
            .route("/linkup-check", web::route().to(always_ok))
            .service(
                web::resource("{path:.*}")
                    .guard(guard::Header("upgrade", "websocket"))
                    .to(linkup_ws_request_handler),
            )
            .default_service(web::route().to(linkup_request_handler))
    })
    .bind(("127.0.0.1", LINKUP_LOCALSERVER_PORT))?
    .run()
    .await
}
