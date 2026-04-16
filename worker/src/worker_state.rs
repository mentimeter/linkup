use linkup::Version;
use worker::{Env, kv::KvStore};

#[derive(Clone)]
#[allow(dead_code)]
pub struct CloudflareEnvironemnt {
    pub account_id: String,
    pub tunnel_zone_id: String,
    pub all_zone_ids: Vec<String>,
    pub api_token: String,
    pub worker_token: String,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct WorkerState {
    pub min_supported_client_version: Version,
    pub sessions_kv: KvStore,
    pub tunnels_kv: KvStore,
    pub cloudflare: CloudflareEnvironemnt,
    pub env: Env,
}

impl TryFrom<Env> for WorkerState {
    type Error = worker::Error;

    fn try_from(value: Env) -> Result<Self, Self::Error> {
        let min_supported_client_version = Version::try_from(crate::MIN_SUPPORTED_CLIENT_VERSION)
            .expect("MIN_SUPPORTED_CLIENT_VERSION to be a valid version");

        let sessions_kv = value.kv("LINKUP_SESSIONS")?;
        let tunnels_kv = value.kv("LINKUP_TUNNELS")?;
        let cf_account_id = value.var("CLOUDFLARE_ACCOUNT_ID")?;
        let cf_tunnel_zone_id = value.var("CLOUDFLARE_TUNNEL_ZONE_ID")?;
        let cf_all_zone_ids: Vec<String> = value
            .var("CLOUDLFLARE_ALL_ZONE_IDS")?
            .to_string()
            .split(",")
            .map(String::from)
            .collect();
        let cf_api_token = value.var("CLOUDFLARE_API_TOKEN")?;
        let worker_token = value.var("WORKER_TOKEN")?;

        let state = WorkerState {
            min_supported_client_version,
            sessions_kv,
            tunnels_kv,
            cloudflare: CloudflareEnvironemnt {
                account_id: cf_account_id.to_string(),
                tunnel_zone_id: cf_tunnel_zone_id.to_string(),
                all_zone_ids: cf_all_zone_ids,
                api_token: cf_api_token.to_string(),
                worker_token: worker_token.to_string(),
            },
            env: value,
        };

        Ok(state)
    }
}
