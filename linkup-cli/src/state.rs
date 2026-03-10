use std::{
    env,
    fmt::{self, Display, Formatter},
    fs,
};

use anyhow::Context;
use rand::distr::{Alphanumeric, SampleString};
use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use linkup::{Domain, Session, SessionService, UpdateSessionRequest};

use crate::{
    linkup_file_path, services,
    worker_client::{self, WorkerClient},
    Result, LINKUP_CONFIG_ENV, LINKUP_STATE_FILE,
};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct State {
    pub linkup: LinkupState,
    pub domains: Vec<Domain>,
    pub services: Vec<LocalService>,
}

impl State {
    pub fn load() -> anyhow::Result<Self> {
        let state_file_path = linkup_file_path(LINKUP_STATE_FILE);
        let content = fs::read_to_string(&state_file_path)
            .with_context(|| format!("Failed to read state file on {:?}", &state_file_path))?;

        serde_yaml::from_str(&content).context("Failed to parse state file")
    }

    pub fn save(&mut self) -> Result<()> {
        if cfg!(test) {
            return Ok(());
        }

        let yaml_string =
            serde_yaml::to_string(self).context("Failed to serialize the state into YAML")?;

        let state_file_location = linkup_file_path(LINKUP_STATE_FILE);
        fs::write(&state_file_location, yaml_string).with_context(|| {
            format!("Failed to write the state file to {state_file_location:?}")
        })?;

        Ok(())
    }

    pub fn should_use_tunnel(&self) -> bool {
        self.linkup.tunnel.is_some()
    }

    pub fn get_tunnel_url(&self) -> Url {
        match &self.linkup.tunnel {
            Some(url) => url.clone(),
            None => {
                let mut remote = self.linkup.worker_url.clone();
                remote.set_path("/linkup/no-tunnel");
                remote
            }
        }
    }

    pub fn domain_strings(&self) -> Vec<String> {
        self.domains
            .iter()
            .map(|storable_domain| storable_domain.domain.clone())
            .collect::<Vec<String>>()
    }

    pub fn exists() -> bool {
        linkup_file_path(LINKUP_STATE_FILE).exists()
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LinkupState {
    pub session_name: String,
    pub session_token: String,
    pub worker_url: Url,
    pub worker_token: String,
    pub config_path: String,
    pub tunnel: Option<Url>,
    #[serde(
        default,
        serialize_with = "linkup::serde_ext::serialize_opt_vec_regex",
        deserialize_with = "linkup::serde_ext::deserialize_opt_vec_regex"
    )]
    pub cache_routes: Option<Vec<Regex>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LocalService {
    pub current: ServiceTarget,

    #[serde(flatten)]
    pub config: linkup::config::ServiceConfig,
}

impl LocalService {
    pub fn current_url(&self) -> Url {
        match self.current {
            ServiceTarget::Local => self.config.local.clone(),
            ServiceTarget::Remote => self.config.remote.clone(),
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
pub enum ServiceTarget {
    Local,
    Remote,
}

impl Display for ServiceTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ServiceTarget::Local => write!(f, "local"),
            ServiceTarget::Remote => write!(f, "remote"),
        }
    }
}

#[derive(Debug)]
pub struct ServerConfig {
    pub local: Session,
    pub remote: Session,
}

pub fn config_to_state(
    config: linkup::config::Config,
    config_path: String,
    no_tunnel: bool,
) -> State {
    let random_token = Alphanumeric.sample_string(&mut rand::rng(), 16);

    let tunnel = match no_tunnel {
        true => None,
        false => Some(Url::parse("http://tunnel-not-yet-set").expect("default url parses")),
    };

    let linkup = LinkupState {
        session_name: String::new(),
        session_token: random_token,
        worker_token: config.linkup.worker_token,
        config_path,
        worker_url: config.linkup.worker_url,
        tunnel,
        cache_routes: config.linkup.cache_routes,
    };

    let services = config
        .services
        .into_iter()
        .map(|service_config| LocalService {
            config: service_config.clone(),
            current: ServiceTarget::Remote,
        })
        .collect::<Vec<LocalService>>();

    let domains = config.domains;

    State {
        linkup,
        domains,
        services,
    }
}

pub fn config_path(config_arg: &Option<String>) -> Result<String> {
    match config_arg {
        Some(path) => {
            let absolute_path = fs::canonicalize(path)
                .with_context(|| format!("Unable to resolve absolute path for {path:?}"))?;

            Ok(absolute_path.to_string_lossy().into_owned())
        }
        None => {
            let path = env::var(LINKUP_CONFIG_ENV).context(
                "No config argument provided and LINKUP_CONFIG environment variable not set",
            )?;

            let absolute_path = fs::canonicalize(&path)
                .with_context(|| format!("Unalbe to resolve absolute path for {path:?}"))?;

            Ok(absolute_path.to_string_lossy().into_owned())
        }
    }
}

pub fn get_config(config_path: &str) -> Result<linkup::config::Config> {
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file {config_path:?}"))?;

    serde_yaml::from_str(&content)
        .with_context(|| "Failed to deserialize config file {config_path:?}")
}

// This method gets the local state and uploads it to both the local linkup server and
// the remote linkup server (worker).
pub async fn upload_state(state: &State) -> Result<String> {
    let local_url = services::LocalServer::url();

    let server_config = ServerConfig::from(state);
    let session_name = &state.linkup.session_name;

    let server_session_name = upload_session_to_server(
        &state.linkup.worker_url,
        &state.linkup.worker_token,
        session_name,
        server_config.remote,
    )
    .await?;

    let local_session_name = upload_session_to_server(
        &local_url,
        &state.linkup.worker_token,
        &server_session_name,
        server_config.local,
    )
    .await?;

    if server_session_name != local_session_name {
        log::error!(
            "Local session has name: {} and remote has name: {}",
            &local_session_name,
            &server_session_name
        );

        return Err(worker_client::Error::InconsistentState.into());
    }

    Ok(server_session_name)
}

async fn upload_session_to_server(
    linkup_url: &Url,
    worker_token: &str,
    desired_name: &str,
    session: Session,
) -> Result<String, worker_client::Error> {
    let session_update_req = UpdateSessionRequest {
        session_token: session.session_token,
        desired_name: desired_name.to_string(),
        services: session.services,
        domains: session.domains,
        cache_routes: session.cache_routes,
    };

    let session_name = WorkerClient::new(linkup_url, worker_token)
        .linkup(&session_update_req)
        .await?;

    Ok(session_name)
}

impl From<&State> for ServerConfig {
    fn from(state: &State) -> Self {
        let local_server_services = state
            .services
            .iter()
            .map(|service| SessionService {
                name: service.config.name.clone(),
                location: if service.current == ServiceTarget::Remote {
                    service.config.remote.clone()
                } else {
                    service.config.local.clone()
                },
                rewrites: service.config.rewrites.clone(),
            })
            .collect::<Vec<SessionService>>();

        let remote_server_services = state
            .services
            .iter()
            .map(|service| SessionService {
                name: service.config.name.clone(),
                location: if service.current == ServiceTarget::Remote {
                    service.config.remote.clone()
                } else {
                    state.get_tunnel_url()
                },
                rewrites: service.config.rewrites.clone(),
            })
            .collect::<Vec<SessionService>>();

        let local_storable_session = Session {
            session_token: state.linkup.session_token.clone(),
            services: local_server_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        };

        let remote_storable_session = Session {
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

pub fn managed_domains(state: Option<&State>, cfg_path: &Option<String>) -> Vec<String> {
    let config_domains = match config_path(cfg_path).ok() {
        Some(cfg_path) => match get_config(&cfg_path) {
            Ok(config) => Some(
                config
                    .domains
                    .iter()
                    .map(|storable_domain| storable_domain.domain.clone())
                    .collect::<Vec<String>>(),
            ),
            Err(_) => None,
        },
        None => None,
    };

    let state_domains = state.map(|state| state.domain_strings());

    let mut domain_set = std::collections::HashSet::new();

    if let Some(domains) = config_domains {
        domain_set.extend(domains);
    }

    if let Some(domains) = state_domains {
        domain_set.extend(domains);
    }

    domain_set.into_iter().collect()
}

pub fn top_level_domains(domains: &[String]) -> Vec<String> {
    domains
        .iter()
        .filter(|&domain| {
            !domains
                .iter()
                .any(|other_domain| other_domain != domain && domain.ends_with(other_domain))
        })
        .cloned()
        .collect::<Vec<String>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const CONF_STR: &str = r#"
linkup:
  worker_url: https://remote-linkup.example.com
  worker_token: test_token_123
services:
  - name: frontend
    remote: http://remote-service1.example.com
    local: http://localhost:8000
    rewrites:
      - source: /foo/(.*)
        target: /bar/$1
  - name: backend
    remote: http://remote-service2.example.com
    local: http://localhost:8001
    directory: ../backend
    health:
      path: /health
      statuses: [200, 304]
domains:
  - domain: example.com
    default_service: frontend
    routes:
      - path: /api/v1/.*
        service: backend
  - domain: api.example.com
    default_service: backend
    "#;

    #[test]
    fn test_config_to_state() {
        let input_str = String::from(CONF_STR);
        let config = serde_yaml::from_str(&input_str).unwrap();
        let local_state = config_to_state(config, "./path/to/config.yaml".to_string(), false);

        assert_eq!(local_state.linkup.config_path, "./path/to/config.yaml");

        assert_eq!(
            local_state.linkup.worker_url,
            Url::parse("https://remote-linkup.example.com").unwrap()
        );
        assert_eq!(
            local_state.linkup.worker_token,
            String::from("test_token_123"),
        );

        assert_eq!(local_state.services.len(), 2);
        assert_eq!(local_state.services[0].config.name, "frontend");
        assert_eq!(
            local_state.services[0].config.remote,
            Url::parse("http://remote-service1.example.com").unwrap()
        );
        assert_eq!(
            local_state.services[0].config.local,
            Url::parse("http://localhost:8000").unwrap()
        );
        assert_eq!(local_state.services[0].current, ServiceTarget::Remote);
        assert!(matches!(local_state.services[0].config.health, None));

        assert_eq!(
            local_state.services[0]
                .config
                .rewrites
                .as_ref()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(local_state.services[1].config.name, "backend");
        assert_eq!(
            local_state.services[1].config.remote,
            Url::parse("http://remote-service2.example.com").unwrap()
        );
        assert_eq!(
            local_state.services[1].config.local,
            Url::parse("http://localhost:8001").unwrap()
        );
        assert!(matches!(local_state.services[1].config.rewrites, None));
        assert_eq!(
            local_state.services[1].config.directory,
            Some("../backend".to_string())
        );
        assert!(local_state.services[1].config.health.is_some());
        let health = local_state.services[1].config.health.as_ref().unwrap();
        assert_eq!(health.path, Some("/health".to_string()));
        assert_eq!(health.statuses, Some(vec![200, 304]));

        assert_eq!(local_state.domains.len(), 2);
        assert_eq!(local_state.domains[0].domain, "example.com");
        assert_eq!(local_state.domains[0].default_service, "frontend");
        assert!(local_state.domains[0].routes.is_some());
    }
}
