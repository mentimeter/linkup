use std::time::Duration;

use axum::{
    extract::{self, State},
    response::IntoResponse,
    routing::{get, put},
    Router,
};
use http::StatusCode;
use worker::{console_error, console_log, durable_object, Env};

use crate::LinkupState;

pub fn router() -> Router<LinkupState> {
    Router::new()
        .route(
            "/linkup/certificate-cache/locks/{key}",
            get(get_lock_handler).delete(delete_lock_handler),
        )
        .route(
            "/linkup/certificate-cache/locks/{key}/touch",
            put(touch_lock_handler),
        )
}

#[worker::send]
async fn get_lock_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
) -> impl IntoResponse {
    console_log!("Trying to acquire lock on key: {}", key);

    match fetch_lock(&state.env, &key).await {
        Ok(res) if res.status_code() == 200 => (StatusCode::OK).into_response(),
        Ok(res) if res.status_code() == 423 => (StatusCode::LOCKED).into_response(),
        Ok(res) => {
            console_error!(
                "Durable object responded with unsupported HTTP status while ACQUIRING lock with key '{}': {}",
                &key,
                res.status_code()
            );

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
        Err(error) => {
            console_error!("Error ACQUIRING lock with key '{}': {}", &key, error);

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

#[worker::send]
async fn touch_lock_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
) -> impl IntoResponse {
    console_log!("Touching lock with key: {}", key);

    match fetch_touch(&state.env, &key).await {
        Ok(res) if res.status_code() == 200 => (StatusCode::OK).into_response(),
        Ok(res) => {
            console_error!(
                "Durable object responded with unsupported HTTP status while TOUCHING lock with key '{}': {}",
                &key,
                res.status_code()
            );

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
        Err(error) => {
            console_error!("Error TOUCHING lock with key '{}': {}", &key, error);

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

#[worker::send]
async fn delete_lock_handler(
    State(state): State<LinkupState>,
    extract::Path(key): extract::Path<String>,
) -> impl IntoResponse {
    console_log!("Unlocking lock with key: {}", key);

    match fetch_unlock(&state.env, &key).await {
        Ok(res) if res.status_code() == 200 => (StatusCode::OK).into_response(),
        Ok(res) => {
            console_error!(
                "Durable object responded with unsupported HTTP status while UNLOCKING lock with key '{}': {}",
                &key,
                res.status_code()
            );

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
        Err(error) => {
            console_error!("Error UNLOCKING lock with key '{}': {}", &key, error);

            (StatusCode::INTERNAL_SERVER_ERROR).into_response()
        }
    }
}

fn get_stub(env: &Env, key: &str) -> worker::Result<worker::Stub> {
    let namespace = env.durable_object("CERTIFICATE_LOCKS")?;
    namespace.id_from_name(key)?.get_stub()
}

async fn fetch_lock(env: &Env, key: &str) -> worker::Result<worker::Response> {
    let stub = get_stub(env, key)?;
    stub.fetch_with_str("http://fake_url.com/lock").await
}

async fn fetch_touch(env: &Env, key: &str) -> worker::Result<worker::Response> {
    let stub = get_stub(env, key)?;
    stub.fetch_with_str("http://fake_url.com/touch").await
}

async fn fetch_unlock(env: &Env, key: &str) -> worker::Result<worker::Response> {
    let stub = get_stub(env, key)?;
    stub.fetch_with_str("http://fake_url.com/unlock").await
}

#[durable_object]
pub struct CertificateStoreLock {
    state: worker::State,
    locked: bool,
    last_touched: worker::Date,
}

impl CertificateStoreLock {
    pub async fn lock(&mut self) -> worker::Result<worker::Response> {
        if self.locked {
            Ok(worker::Response::builder().with_status(423).empty())
        } else {
            self.state
                .storage()
                .set_alarm(Duration::from_secs(3))
                .await?;

            self.locked = true;
            self.last_touched = worker::Date::now();

            worker::Response::empty()
        }
    }

    pub async fn touch(&mut self) -> worker::Result<worker::Response> {
        self.last_touched = worker::Date::now();

        worker::Response::empty()
    }

    pub async fn unlock(&mut self) -> worker::Result<worker::Response> {
        self.locked = false;

        if let Err(error) = self.state.storage().delete_alarm().await {
            console_log!("Error deleting alarm on unlock: {}", error);
        }

        worker::Response::empty()
    }
}

#[durable_object]
impl DurableObject for CertificateStoreLock {
    fn new(state: worker::State, _env: Env) -> Self {
        Self {
            state,
            locked: false,
            last_touched: worker::Date::now(),
        }
    }

    async fn fetch(&mut self, req: worker::Request) -> worker::Result<worker::Response> {
        match req.path().as_str() {
            "/lock" => self.lock().await,
            "/touch" => self.touch().await,
            "/unlock" => self.unlock().await,
            _ => Ok(worker::Response::builder().with_status(404).empty()),
        }
    }

    async fn alarm(&mut self) -> worker::Result<worker::Response> {
        if worker::Date::now().as_millis() - self.last_touched.as_millis() > 5000 {
            self.locked = false;
        } else if let Err(error) = self.state.storage().set_alarm(Duration::from_secs(3)).await {
            console_log!("Error setting alarm: {}", error);

            // NOTE(augustoccesar)[2025-02-25]: If we fail to set the next alarm, instantly unlock the
            //   lock to avoid ending on a deadlock.
            self.locked = false;
        }

        worker::Response::empty()
    }
}
