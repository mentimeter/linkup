#![cfg(feature = "worker")]

use tower_service::Service;

#[worker::event(fetch)]
async fn fetch(
    req: worker::HttpRequest,
    _env: worker::Env,
    _ctx: worker::Context,
) -> worker::Result<axum::http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();

    Ok(crate::router().call(req).await?)
}
