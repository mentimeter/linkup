use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TunnelData {
    pub account_id: String,
    pub name: String,
    pub url: String,
    pub id: String,
    pub secret: String,
    pub last_started: u64,
}

#[derive(Serialize)]
pub struct GetTunnelRequest {
    pub session_name: String,
}
