use worker::*;
use serpress::*;

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

    // let head = req.headers();
    // head.

    let server_conf = new_server_config(String::from(r#"
    services:
        - name: core
    domains:
        - domain: "serpress.dev" 
    "#));

    let resp = match server_conf {
        Ok(conf) => "all good",
        Err(_) => "no good",
    };

    Response::ok(resp)
}