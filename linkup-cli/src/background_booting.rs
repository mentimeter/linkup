use linkup::{StorableService, StorableSession, UpdateSessionRequest};
use url::Url;

use crate::local_config::{LocalState, ServiceTarget};
use crate::worker_client::WorkerClient;
use crate::CliError;

// TODO(augustoccesar)[2024-12-06]: This method might need a better name and maybe live somewhere else?
pub fn load_config(
    url: &Url,
    desired_name: &str,
    config: StorableSession,
) -> Result<String, CliError> {
    let session_update_req = UpdateSessionRequest {
        session_token: config.session_token,
        desired_name: desired_name.to_string(),
        services: config.services,
        domains: config.domains,
        cache_routes: config.cache_routes,
    };

    let content = WorkerClient::new(url)
        .linkup(&session_update_req)
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    Ok(content)
}

pub struct ServerConfig {
    pub local: StorableSession,
    pub remote: StorableSession,
}

impl From<&LocalState> for ServerConfig {
    fn from(state: &LocalState) -> Self {
        let local_server_services = state
            .services
            .iter()
            .map(|service| StorableService {
                name: service.name.clone(),
                location: if service.current == ServiceTarget::Remote {
                    service.remote.clone()
                } else {
                    service.local.clone()
                },
                rewrites: Some(service.rewrites.clone()),
            })
            .collect::<Vec<StorableService>>();

        let remote_server_services = state
            .services
            .iter()
            .map(|service| StorableService {
                name: service.name.clone(),
                location: if service.current == ServiceTarget::Remote {
                    service.remote.clone()
                } else {
                    state.get_tunnel_url()
                },
                rewrites: Some(service.rewrites.clone()),
            })
            .collect::<Vec<StorableService>>();

        let local_storable_session = StorableSession {
            session_token: state.linkup.session_token.clone(),
            services: local_server_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        };

        let remote_storable_session = StorableSession {
            session_token: state.linkup.session_token.clone(),
            services: remote_server_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        };

        ServerConfig {
            local: local_storable_session,
            remote: remote_storable_session,
        }
    }
}

impl<'a> From<&'a ServerConfig> for (&'a StorableSession, &'a StorableSession) {
    fn from(config: &'a ServerConfig) -> Self {
        (&config.local, &config.remote)
    }
}
