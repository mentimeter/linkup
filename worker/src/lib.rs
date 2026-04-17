use axum::{body::Body, response::Response};
use tower_service::Service;
use worker::{Env, HttpRequest, console_error, console_log, console_warn, event};

use crate::{router::router, tunnel::TunnelData, worker_state::WorkerState};

mod handlers;
mod http_error;
mod kv_store;
mod router;
mod tunnel;
mod worker_state;
mod ws;

pub(crate) const SEVEN_DAYS_MILLIS: u64 = 7 * 24 * 60 * 60 * 1000;
pub(crate) const MIN_SUPPORTED_CLIENT_VERSION: &str = "2.1.0";

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: worker::Context,
) -> Result<Response<Body>, worker::Error> {
    console_error_panic_hook::set_once();

    let state = WorkerState::try_from(env)?;

    Ok(router(state).call(req).await?)
}

#[event(scheduled)]
async fn scheduled(_event: worker::ScheduledEvent, env: Env, _ctx: worker::ScheduleContext) {
    let state =
        WorkerState::try_from(env).expect("WorkerState to be buildable from worker environment");

    cleanup_unused_sessions(&state).await;
}

async fn cleanup_unused_sessions(state: &WorkerState) {
    let tunnels_keys = state
        .tunnels_kv
        .list()
        .limit(1000)
        .execute()
        .await
        .unwrap()
        .keys;

    let now = worker::Date::now();

    for key in tunnels_keys {
        match state.tunnels_kv.get(&key.name).json::<TunnelData>().await {
            Ok(Some(tunnel_data)) => {
                let last_started =
                    worker::Date::from(worker::DateInit::Millis(tunnel_data.last_started));

                if now.as_millis() - last_started.as_millis() > SEVEN_DAYS_MILLIS {
                    console_log!(
                        "Deleting unused tunnel '{}'. Last used: {}",
                        &key.name,
                        &last_started
                    );

                    match tunnel::delete_tunnel(
                        &state.cloudflare.api_token,
                        &state.cloudflare.account_id,
                        &state.cloudflare.tunnel_zone_id,
                        &tunnel_data.id,
                    )
                    .await
                    {
                        Ok(_) => {
                            if let Err(error) = state.tunnels_kv.delete(&key.name).await {
                                console_error!("Failed to delete tunnel info from KV: {}", error);
                            }
                        }
                        Err(error) => {
                            console_error!("Failed to delete tunnel: {}", error);
                        }
                    }
                }
            }
            Ok(None) => {
                console_warn!("Tunnel data for key '{}' not found.", &key.name);
            }
            Err(error) => {
                console_error!("Failed to deserialize tunnel data: {}", error.to_string());
            }
        }
    }
}

pub(crate) fn cloudflare_client(api_token: &str) -> cloudflare::framework::async_api::Client {
    cloudflare::framework::async_api::Client::new(
        cloudflare::framework::auth::Credentials::UserAuthToken {
            token: api_token.to_string(),
        },
        cloudflare::framework::HttpApiClientConfig::default(),
        cloudflare::framework::Environment::Production,
    )
    .expect("Cloudflare API Client to have been created")
}

pub fn generate_secret() -> String {
    let mut random_bytes = [0u8; 32];
    getrandom::fill(&mut random_bytes).unwrap();

    base64::Engine::encode(&base64::prelude::BASE64_STANDARD, random_bytes)
}
