
use std::io;

use actix_web::{Responder, HttpServer, App, web, HttpResponse, middleware, HttpRequest};
use serpress::MemorySessionStore;

use crate::SERPRESS_PORT;

async fn serpress_handler(
  session_store: web::Data<MemorySessionStore>,
  req: HttpRequest,
) -> impl Responder {
  // Your handler logic using session_store
  HttpResponse::Ok().body("Serpress handler")
}

async fn fallback_handler(
  session_store: web::Data<MemorySessionStore>,
  req: HttpRequest,
) -> impl Responder {
  HttpResponse::NotFound().body("Fallback handler")
}

#[actix_web::main]
pub async fn local_serpress_main() -> io::Result<()> {
  let session_store = web::Data::new(MemorySessionStore::new());

  HttpServer::new(move || {
      App::new()
          .app_data(session_store.clone()) // Add shared state
          .wrap(middleware::Logger::default()) // Enable logger
          .route("/serpress", web::post().to(serpress_handler))
          .default_service(web::route().to(fallback_handler))
  })
  .bind(("127.0.0.1", SERPRESS_PORT))?
  .run()
  .await
}