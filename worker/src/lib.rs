use kv_store::KvSessionStore;
use serpress::*;
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

#[event(fetch)]
pub async fn main(req: Request, _env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();

    let store = KvSessionStore::new();

    // Headers to hashmap
    let headers = req.headers();

    // get_request_session

    // get_target_url

    // get_additional_headers

    // let head = req.headers();
    // head.

    let server_conf = new_server_config(String::from(
        r#"
    services:
        - name: core
    domains:
        - domain: "serpress.dev" 
    "#,
    ));

    let resp = match server_conf {
        Ok(conf) => "all good",
        Err(_) => "no good",
    };

    Response::ok(resp)
}
