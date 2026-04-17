use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use http::StatusCode;
use linkup::TunnelData;
use serde::Deserialize;
use worker::console_error;

use crate::{cloudflare_client, http_error::HttpError, tunnel, worker_state::WorkerState};

#[derive(Deserialize)]
pub struct GetTunnelParams {
    session_name: String,
}

#[worker::send]
pub async fn handle_get(
    State(state): State<WorkerState>,
    Query(query): Query<GetTunnelParams>,
) -> impl IntoResponse {
    let kv = state.tunnels_kv;

    let cf_client = cloudflare_client(&state.cloudflare.api_token);
    let tunnel_prefix =
        match cloudflare::linkup::tunnel_prefix(&cf_client, &state.cloudflare.tunnel_zone_id).await
        {
            Ok(prefix) => prefix,
            Err(error) => {
                console_error!("Failed resolve tunnel prefix: {}", error);

                return HttpError::new(
                    "Failed to generate tunnel.".to_string(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response();
            }
        };

    let tunnel_name = format!("{}{}", tunnel_prefix, query.session_name);
    let tunnel_data: Option<TunnelData> = kv.get(&tunnel_name).json().await.unwrap();

    match tunnel_data {
        Some(mut tunnel_data) => {
            tunnel_data.last_started = worker::Date::now().as_millis();
            kv.put(&tunnel_name, &tunnel_data)
                .unwrap()
                .execute()
                .await
                .unwrap();

            Json(tunnel_data).into_response()
        }
        None => {
            let tunnel_data = tunnel::create_tunnel(
                &state.cloudflare.api_token,
                &state.cloudflare.account_id,
                &state.cloudflare.tunnel_zone_id,
                &tunnel_name,
            )
            .await
            .unwrap();

            kv.put(&tunnel_name, &tunnel_data)
                .unwrap()
                .execute()
                .await
                .unwrap();

            Json(tunnel_data).into_response()
        }
    }
}
