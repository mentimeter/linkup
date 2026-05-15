use std::{
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use rand::distr::{Alphanumeric, SampleString};
use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use linkup::{Domain, Session, SessionKind, SessionService};

use crate::{LINKUP_STATE_FILE, Result, config::load_config_with_override, linkup_file_path};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct State {
    pub linkup: LinkupState,
    pub domains: Vec<Domain>,
    pub services: Vec<LocalService>,
}

impl State {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from_path(&state_file_path(None))
    }

    pub fn load_with_suffix(suffix: &str) -> anyhow::Result<Self> {
        Self::load_from_path(&state_file_path(Some(suffix)))
    }

    pub fn load_from_path(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read state file on {:?}", path))?;

        serde_yaml::from_str(&content).context("Failed to parse state file")
    }

    /// Attempts to load a State from a config. If config_override is None, it will
    /// load the config from the environment variable.
    pub fn from_config(config_path: Option<&Path>) -> anyhow::Result<Self> {
        let (config, config_path) = load_config_with_override(config_path)?;

        Ok(config_to_state(config, &config_path))
    }

    pub fn save(&mut self) -> Result<()> {
        self.save_to_path(&state_file_path(None))
    }

    pub fn save_with_suffix(&self, suffix: &str) -> Result<()> {
        self.save_to_path(&state_file_path(Some(suffix)))
    }

    pub fn delete_with_suffix(suffix: &str) -> Result<()> {
        let path = state_file_path(Some(suffix));

        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete state file {:?}", path))?;
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
                let mut remote = self.linkup.worker_url.clone();
                remote.set_path("/linkup/no-tunnel");
                remote
            }
        }
    }

    pub fn domain_strings(&self) -> Vec<String> {
        self.domains
            .iter()
            .map(|domain| domain.domain.clone())
            .collect::<Vec<String>>()
    }

    pub fn exists() -> bool {
        state_file_path(None).exists()
    }

    fn save_to_path(&self, path: &std::path::Path) -> Result<()> {
        if cfg!(test) {
            return Ok(());
        }

        let yaml_string =
            serde_yaml::to_string(self).context("Failed to serialize the state into YAML")?;

        fs::write(path, yaml_string)
            .with_context(|| format!("Failed to write the state file to {:?}", path))?;

        Ok(())
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
    #[serde(default)]
    pub kind: SessionKind,
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

impl From<&State> for Session {
    fn from(state: &State) -> Self {
        let session_services = state
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
            .collect::<Vec<_>>();

        Session {
            kind: state.linkup.kind.clone(),
            session_token: state.linkup.session_token.clone(),
            services: session_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        }
    }
}

pub fn managed_domains(state: Option<&State>, cfg_path: Option<&Path>) -> Vec<String> {
    let config_domains = load_config_with_override(cfg_path)
        .map(|(config, _)| {
            config
                .domains
                .iter()
                .map(|domain| domain.domain.clone())
                .collect::<Vec<String>>()
        })
        .ok();

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

pub fn find_isolated_suffixes() -> Vec<String> {
    let prefix = format!("{}-", LINKUP_STATE_FILE);

    list_state_files()
        .iter()
        .filter_map(|file_path| {
            file_path
                .file_name()
                .and_then(|file_name| file_name.to_str())
        })
        .filter(|file_name| *file_name != LINKUP_STATE_FILE)
        .filter_map(|file_name| file_name.strip_prefix(&prefix))
        .map(|stripped_file_name| stripped_file_name.to_string())
        .collect()
}

pub fn list_state_files() -> Vec<PathBuf> {
    fs::read_dir(crate::linkup_dir_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let file_name = entry.file_name();
                    let file_name = file_name.to_str()?;

                    if !file_name.starts_with(LINKUP_STATE_FILE) {
                        return None;
                    }

                    Some(entry.path())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn state_file_path(suffix: Option<&str>) -> std::path::PathBuf {
    match suffix {
        None => linkup_file_path(LINKUP_STATE_FILE),
        Some(suffix) => linkup_file_path(&format!("{}-{}", LINKUP_STATE_FILE, suffix)),
    }
}

fn config_to_state(config: linkup::config::Config, config_path: &Path) -> State {
    let random_token = Alphanumeric.sample_string(&mut rand::rng(), 16);

    let linkup = LinkupState {
        session_name: String::new(),
        session_token: random_token,
        worker_token: config.linkup.worker_token,
        config_path: config_path.to_string_lossy().to_string(),
        worker_url: config.linkup.worker_url,
        tunnel: Some(Url::parse("http://tunnel-not-yet-set").expect("default url parses")),
        kind: SessionKind::Tunneled,
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

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

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
        let local_state =
            config_to_state(config, &PathBuf::from_str("./path/to/config.yaml").unwrap());

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
        assert!(local_state.services[0].config.health.is_none());

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
        assert!(local_state.services[1].config.rewrites.is_none());
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

    #[test]
    fn test_state_parses_null_optional_fields() {
        let yaml = r#"
linkup:
  session_name: test-session
  session_token: abc123
  worker_url: https://worker.example.com
  worker_token: token
  config_path: /path/to/config
services:
- current: Remote
  name: null-rewrites
  remote: https://auth.example.com/
  local: http://localhost:3030/
  rewrites: null
  health:
    path: /health
    statuses: null
- current: Remote
  name: empty-rewrites
  remote: https://auth.example.com/
  local: http://localhost:3030/
  rewrites: []
- current: Remote
  name: absent-rewrites
  remote: https://auth.example.com/
  local: http://localhost:3030/
domains: []
"#;

        let state: State =
            serde_yaml::from_str(yaml).expect("state with null/empty/absent rewrites should parse");

        assert!(state.services[0].config.rewrites.is_none(), "null -> None");
        assert!(
            state.services[0]
                .config
                .health
                .as_ref()
                .unwrap()
                .statuses
                .is_none(),
            "null statuses -> None"
        );

        assert_eq!(
            state.services[1].config.rewrites.as_ref().unwrap().len(),
            0,
            "[] -> Some([])"
        );

        assert!(
            state.services[2].config.rewrites.is_none(),
            "absent -> None"
        );
    }
}
