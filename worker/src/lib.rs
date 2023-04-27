use kv_store::KvSessionStore;
use linkup::*;
use worker::{kv::KvStore, *};

mod kv_store;
mod utils;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or_else(|| "unknown region".into())
    );
}

async fn linkup_config_handler(req: Request) -> worker::Result<Response> {
    // let store = KvSessionStore::new();
    Response::ok("yoyo")
}

async fn linkup_request_handler(req: Request) -> worker::Result<Response> {
    // let store = KvSessionStore::new();
    Response::ok("ajaja")
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    Response::ok("Hello, world!")

    // let router = Router::new();

    // router.post("/linkup", |req, _ctx| async move {
    //         linkup_config_handler(req).await
    //     })
    //     .on("/**", |req, _ctx| async move {
    //         linkup_request_handler(req).await
    //     })
    //     .run(req, env).await
}
