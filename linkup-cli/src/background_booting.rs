use std::thread;
use std::time::{Duration, Instant};

use hickory_resolver::config::*;
use hickory_resolver::proto::rr::RecordType;
use hickory_resolver::Resolver;

use reqwest::StatusCode;

use linkup::{StorableService, StorableSession, UpdateSessionRequest};
use url::Url;

use crate::local_config::{LocalState, ServiceTarget};
use crate::worker_client::WorkerClient;
use crate::CliError;
use crate::{services, LINKUP_LOCALSERVER_PORT};

#[cfg_attr(test, mockall::automock)]
pub trait BackgroundServices {
    fn boot_linkup_server(&self, state: LocalState) -> Result<LocalState, CliError>;
    fn boot_local_dns(&self, domains: Vec<String>, session_name: String) -> Result<(), CliError>;
}

pub struct LocalBackgroundServices;

impl BackgroundServices for LocalBackgroundServices {
    fn boot_linkup_server(&self, mut state: LocalState) -> Result<LocalState, CliError> {
        // let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        //     .expect("linkup url invalid");

        // if is_local_server_started().is_err() {
        //     println!("Starting linkup local server...");
        //     // start_local_server()?;
        // } else {
        //     println!("Linkup local server was already running.. Try stopping linkup first if you have problems.");
        // }

        // wait_till_ok(format!("{}linkup-check", local_url))?;

        // let server_config = ServerConfig::from(&state);

        // let server_session_name = load_config(
        //     &state.linkup.remote,
        //     &state.linkup.session_name,
        //     server_config.remote,
        // )?;
        // let local_session_name =
        //     load_config(&local_url, &server_session_name, server_config.local)?;

        // if server_session_name != local_session_name {
        //     return Err(CliError::InconsistentState);
        // }

        // state.linkup.session_name = server_session_name;
        // state.save()?;

        // Ok(state)
        unimplemented!("deprecated")
    }

    fn boot_local_dns(&self, domains: Vec<String>, session_name: String) -> Result<(), CliError> {
        // services::caddy::start(domains.clone())?;
        // services::dnsmasq::start(domains, session_name)?;

        // Ok(())
        unimplemented!("deprecated")
    }
}

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
