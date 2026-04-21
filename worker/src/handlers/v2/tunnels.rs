// use axum::{
//     Json,
//     extract::{Path, State},
//     response::IntoResponse,
// };
// use http::StatusCode;
// use linkup::TunnelData;
// use worker::console_error;

// use crate::{cloudflare_client, http_error::HttpError, worker_state::WorkerState};

// #[worker::send]
// pub async fn find_by_session_name(
//     State(state): State<WorkerState>,
//     Path(session_name): Path<String>,
// ) -> impl IntoResponse {
//     let kv = state.tunnels_kv;

//     let cf_client = cloudflare_client(&state.cloudflare.api_token);
//     let tunnel_prefix =
//         match cloudflare::linkup::tunnel_prefix(&cf_client, &state.cloudflare.tunnel_zone_id).await
//         {
//             Ok(prefix) => prefix,
//             Err(error) => {
//                 console_error!("Failed resolve tunnel prefix: {}", error);

//                 return HttpError::new(
//                     "Failed resolve tunnel prefix".to_string(),
//                     StatusCode::INTERNAL_SERVER_ERROR,
//                 )
//                 .into_response();
//             }
//         };

//     let tunnel_name = format!("{}{}", tunnel_prefix, session_name);

//     match kv.get(&tunnel_name).json::<TunnelData>().await {
//         Ok(Some(tunnel_data)) => Json(tunnel_data).into_response(),
//         Ok(None) => {
//             HttpError::new("Tunnel not found".to_string(), StatusCode::NOT_FOUND).into_response()
//         }
//         Err(error) => {
//             console_error!("Failed to get tunnel data: {}", error);

//             HttpError::new(
//                 "Failed to get tunnel data".to_string(),
//                 StatusCode::INTERNAL_SERVER_ERROR,
//             )
//             .into_response()
//         }
//     }
// }
