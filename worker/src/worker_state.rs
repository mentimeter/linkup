use linkup::{SessionAllocator, Version};
use worker::{Env, kv::KvStore};

use crate::kv_store::CfWorkerStringStore;

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
    pub session_allocator: SessionAllocator<CfWorkerStringStore>,
    pub tunnels_kv: KvStore,
    pub cloudflare: CloudflareEnvironemnt,
    pub env: Env,
    pub tunnel_prefix: String,
}

impl WorkerState {
    pub async fn load(env: Env) -> Result<Self, worker::Error> {
        let min_supported_client_version = Version::try_from(crate::MIN_SUPPORTED_CLIENT_VERSION)
            .expect("MIN_SUPPORTED_CLIENT_VERSION to be a valid version");

        let sessions_kv = env.kv("LINKUP_SESSIONS")?;
        let tunnels_kv = env.kv("LINKUP_TUNNELS")?;
        let cf_account_id = env.var("CLOUDFLARE_ACCOUNT_ID")?;
        let cf_tunnel_zone_id = env.var("CLOUDFLARE_TUNNEL_ZONE_ID")?.to_string();
        let cf_all_zone_ids: Vec<String> = env
            .var("CLOUDLFLARE_ALL_ZONE_IDS")?
            .to_string()
            .split(",")
            .map(String::from)
            .collect();
        let cf_api_token = env.var("CLOUDFLARE_API_TOKEN")?.to_string();
        let worker_token = env.var("WORKER_TOKEN")?;
        let tunnel_prefix = env.var("TUNNEL_NAME_PREFIX")?.to_string();

        let session_allocator = SessionAllocator::new(CfWorkerStringStore::new(sessions_kv));

        let state = WorkerState {
            min_supported_client_version,
            session_allocator,
            tunnels_kv,
            tunnel_prefix,
            cloudflare: CloudflareEnvironemnt {
                account_id: cf_account_id.to_string(),
                tunnel_zone_id: cf_tunnel_zone_id,
                all_zone_ids: cf_all_zone_ids,
                api_token: cf_api_token,
                worker_token: worker_token.to_string(),
            },
            env,
        };

        Ok(state)
    }
}
