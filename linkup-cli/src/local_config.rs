use std::{
    env,
    fmt::{self, Display, Formatter},
    fs,
};

use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use url::Url;

use linkup::{
    CreatePreviewRequest, StorableDomain, StorableRewrite, StorableService, StorableSession,
    UpdateSessionRequest,
};

use crate::{
    linkup_file_path, services,
    worker_client::{self, WorkerClient},
    CliError, LINKUP_CONFIG_ENV, LINKUP_STATE_FILE,
};

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct LocalState {
    pub linkup: LinkupState,
    pub domains: Vec<StorableDomain>,
    pub services: Vec<LocalService>,
}

impl LocalState {
    pub fn load() -> Result<Self, CliError> {
        if let Err(e) = fs::File::open(linkup_file_path(LINKUP_STATE_FILE)) {
            return Err(CliError::NoState(e.to_string()));
        }

        let content = match fs::read_to_string(linkup_file_path(LINKUP_STATE_FILE)) {
            Ok(content) => content,
            Err(e) => return Err(CliError::NoState(e.to_string())),
        };

        match serde_yaml::from_str(&content) {
            Ok(config) => Ok(config),
            Err(e) => Err(CliError::NoState(e.to_string())),
        }
    }

    pub fn save(&mut self) -> Result<(), CliError> {
        if cfg!(test) {
            return Ok(());
        }
        let yaml_string = match serde_yaml::to_string(self) {
            Ok(yaml) => yaml,
            Err(_) => {
                return Err(CliError::SaveState(
                    "Failed to serialize the state into YAML".to_string(),
                ))
            }
        };

        if fs::write(linkup_file_path(LINKUP_STATE_FILE), yaml_string).is_err() {
            return Err(CliError::SaveState(format!(
                "Failed to write the state file at {}",
                linkup_file_path(LINKUP_STATE_FILE).display()
            )));
        }

        Ok(())
    }

    pub fn should_use_tunnel(&self) -> bool {
        self.linkup.tunnel.is_some()
    }

    pub fn get_tunnel_url(&self) -> Url {
        match &self.linkup.tunnel {
            Some(url) => url.clone(),
            None => {
                let mut remote = self.linkup.remote.clone();
                remote.set_path("/linkup-no-tunnel");
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
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct LinkupState {
    pub session_name: String,
    pub session_token: String,
    pub config_path: String,
    pub remote: Url,
    pub tunnel: Option<Url>,
    pub is_paid: Option<bool>,
    pub cache_routes: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct LocalService {
    pub name: String,
    pub remote: Url,
    pub local: Url,
    pub current: ServiceTarget,
    pub directory: Option<String>,
    pub rewrites: Vec<StorableRewrite>,
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

#[derive(Deserialize, Clone)]
pub struct YamlLocalConfig {
    pub linkup: LinkupConfig,
    pub services: Vec<YamlLocalService>,
    pub domains: Vec<StorableDomain>,
}

impl YamlLocalConfig {
    pub fn top_level_domains(&self) -> Vec<String> {
        self.domains
            .iter()
            .filter(|&d| {
                !self
                    .domains
                    .iter()
                    .any(|other| other.domain != d.domain && d.domain.ends_with(&other.domain))
            })
            .map(|d| d.domain.clone())
            .collect::<Vec<String>>()
    }

    pub fn create_preview_request(&self, services: &[(String, String)]) -> CreatePreviewRequest {
        let services = self
            .services
            .iter()
            .map(|yaml_local_service: &YamlLocalService| {
                let name = yaml_local_service.name.clone();
                let mut location = yaml_local_service.remote.clone();

                for (param_service_name, param_service_url) in services {
                    if param_service_name == &name {
                        location = Url::parse(param_service_url).unwrap();
                    }
                }

                StorableService {
                    name,
                    location,
                    rewrites: yaml_local_service.rewrites.clone(),
                }
            })
            .collect();

        CreatePreviewRequest {
            services,
            domains: self.domains.clone(),
            cache_routes: self.linkup.cache_routes.clone(),
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct LinkupConfig {
    pub remote: Url,
    cache_routes: Option<Vec<String>>,
}

#[derive(Deserialize, Clone)]
pub struct YamlLocalService {
    name: String,
    remote: Url,
    local: Url,
    directory: Option<String>,
    rewrites: Option<Vec<StorableRewrite>>,
}

#[derive(Debug)]
pub struct ServerConfig {
    pub local: StorableSession,
    pub remote: StorableSession,
}

pub fn config_to_state(
    yaml_config: YamlLocalConfig,
    config_path: String,
    no_tunnel: bool,
    is_paid: bool,
) -> LocalState {
    let random_token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();

    let tunnel = match no_tunnel {
        true => None,
        false => Some(Url::parse("http://tunnel-not-yet-set").expect("default url parses")),
    };

    let linkup = LinkupState {
        is_paid: Some(is_paid),
        session_name: String::new(),
        session_token: random_token,
        config_path,
        remote: yaml_config.linkup.remote,
        tunnel,
        cache_routes: yaml_config.linkup.cache_routes,
    };

    let services = yaml_config
        .services
        .into_iter()
        .map(|yaml_service| LocalService {
            name: yaml_service.name,
            remote: yaml_service.remote,
            local: yaml_service.local,
            current: ServiceTarget::Remote,
            directory: yaml_service.directory,
            rewrites: yaml_service.rewrites.unwrap_or_default(),
        })
        .collect::<Vec<LocalService>>();

    let domains = yaml_config.domains;

    LocalState {
        linkup,
        domains,
        services,
    }
}

pub fn config_path(config_arg: &Option<String>) -> Result<String, CliError> {
    match config_arg {
        Some(path) => {
            let absolute_path = fs::canonicalize(path)
                .map_err(|_| CliError::NoConfig("Unable to resolve absolute path".to_string()))?;
            Ok(absolute_path.to_string_lossy().into_owned())
        }
        None => match env::var(LINKUP_CONFIG_ENV) {
            Ok(val) => {
                let absolute_path = fs::canonicalize(val).map_err(|_| {
                    CliError::NoConfig("Unable to resolve absolute path".to_string())
                })?;
                Ok(absolute_path.to_string_lossy().into_owned())
            }
            Err(_) => Err(CliError::NoConfig(
                "No config argument provided and LINKUP_CONFIG environment variable not set"
                    .to_string(),
            )),
        },
    }
}

pub fn get_config(config_path: &str) -> Result<YamlLocalConfig, CliError> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => {
            return Err(CliError::BadConfig(format!(
                "Failed to read the config file at {}",
                config_path
            )))
        }
    };

    let yaml_config: YamlLocalConfig = match serde_yaml::from_str(&content) {
        Ok(config) => config,
        Err(_) => {
            return Err(CliError::BadConfig(format!(
                "Failed to deserialize the config file at {}",
                config_path
            )))
        }
    };

    Ok(yaml_config)
}

// This method gets the local state and uploads it to both the local linkup server and
// the remote linkup server (worker).
pub async fn upload_state(state: &LocalState) -> Result<String, worker_client::Error> {
    let local_url = services::LocalServer::url();

    let server_config = ServerConfig::from(state);
    let session_name = &state.linkup.session_name;

    let server_session_name =
        upload_config_to_server(&state.linkup.remote, session_name, server_config.remote).await?;
    let local_session_name =
        upload_config_to_server(&local_url, &server_session_name, server_config.local).await?;

    if server_session_name != local_session_name {
        log::error!(
            "Local session has name: {} and remote has name: {}",
            &local_session_name,
            &server_session_name
        );

        return Err(worker_client::Error::InconsistentState);
    }

    Ok(server_session_name)
}

async fn upload_config_to_server(
    linkup_url: &Url,
    desired_name: &str,
    config: StorableSession,
) -> Result<String, worker_client::Error> {
    let session_update_req = UpdateSessionRequest {
        session_token: config.session_token,
        desired_name: desired_name.to_string(),
        services: config.services,
        domains: config.domains,
        cache_routes: config.cache_routes,
    };

    let session_name = WorkerClient::new(linkup_url)
        .linkup(&session_update_req)
        .await?;

    Ok(session_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const CONF_STR: &str = r#"
linkup:
  remote: https://remote-linkup.example.com
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
        let yaml_config = serde_yaml::from_str(&input_str).unwrap();
        let local_state = config_to_state(
            yaml_config,
            "./path/to/config.yaml".to_string(),
            false,
            false,
        );

        assert_eq!(local_state.linkup.config_path, "./path/to/config.yaml");

        assert_eq!(
            local_state.linkup.remote,
            Url::parse("https://remote-linkup.example.com").unwrap()
        );

        assert_eq!(local_state.services.len(), 2);
        assert_eq!(local_state.services[0].name, "frontend");
        assert_eq!(
            local_state.services[0].remote,
            Url::parse("http://remote-service1.example.com").unwrap()
        );
        assert_eq!(
            local_state.services[0].local,
            Url::parse("http://localhost:8000").unwrap()
        );
        assert_eq!(local_state.services[0].current, ServiceTarget::Remote);

        assert_eq!(local_state.services[0].rewrites.len(), 1);
        assert_eq!(local_state.services[1].name, "backend");
        assert_eq!(
            local_state.services[1].remote,
            Url::parse("http://remote-service2.example.com").unwrap()
        );
        assert_eq!(
            local_state.services[1].local,
            Url::parse("http://localhost:8001").unwrap()
        );
        assert_eq!(local_state.services[1].rewrites.len(), 0);
        assert_eq!(
            local_state.services[1].directory,
            Some("../backend".to_string())
        );

        assert_eq!(local_state.domains.len(), 2);
        assert_eq!(local_state.domains[0].domain, "example.com");
        assert_eq!(local_state.domains[0].default_service, "frontend");
        assert!(local_state.domains[0].routes.is_some());
    }
}
