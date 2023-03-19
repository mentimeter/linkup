use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};
use thiserror::Error;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone)]
pub struct ServerConfig {
    pub services: HashMap<String, Service>,
    pub domains: HashMap<String, Domain>,
    pub domain_selection_order: Vec<String>,
}

#[derive(Clone)]
pub struct Service {
    pub origin: Url,
    pub path_modifiers: Vec<PathModifier>,
}

#[derive(Clone)]
pub struct PathModifier {
    pub source: Regex,
    pub target: String,
}

#[derive(Clone)]
pub struct Domain {
    pub default_service: String,
    pub routes: Vec<Route>,
}

#[derive(Clone)]
pub struct Route {
    pub path: Regex,
    pub service: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlServerConfigPost {
    pub desired_name: String,
    pub services: Vec<YamlServerService>,
    pub domains: Vec<YamlDomain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlServerConfig {
    pub services: Vec<YamlServerService>,
    pub domains: Vec<YamlDomain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlServerService {
    pub name: String,
    pub location: Url,
    pub path_modifiers: Option<Vec<YamlPathModifier>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YamlPathModifier {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YamlDomain {
    pub domain: String,
    pub default_service: String,
    pub routes: Option<Vec<YamlRoute>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct YamlRoute {
    pub path: String,
    pub service: String,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("serpress config format error: {0}")]
    Format(#[from] serde_yaml::Error),
    #[error("no such service: {0}")]
    NoSuchService(String),
    #[error("invalid regex: {0}, {0}")]
    InvalidRegex(String, regex::Error),
    #[error("domain config error")]
    DomainConfig,
    #[error("invalid url: {0}")]
    InvalidURL(String),
    #[error("empty config")]
    Empty,
}

pub fn new_server_config(input_yaml_conf: String) -> Result<ServerConfig, ConfigError> {
    let yaml_config_res: Result<YamlServerConfig, serde_yaml::Error> =
        serde_yaml::from_str(&input_yaml_conf);
    match yaml_config_res {
        Err(e) => Err(ConfigError::Format(e)),
        Ok(c) => convert_server_config(c),
    }
}

pub fn new_server_config_post(
    input_yaml_conf: String,
) -> Result<(String, ServerConfig), ConfigError> {
    let yaml_config_post_res: Result<YamlServerConfigPost, serde_yaml::Error> =
        serde_yaml::from_str(&input_yaml_conf);
    match yaml_config_post_res {
        Err(e) => Err(ConfigError::Format(e)),
        Ok(c) => {
            let server_conf = convert_server_config(YamlServerConfig {
                services: c.services,
                domains: c.domains,
            });

            match server_conf {
                Err(e) => Err(e),
                Ok(sc) => Ok((c.desired_name, sc)),
            }
        }
    }
}

fn convert_server_config(yaml_config: YamlServerConfig) -> Result<ServerConfig, ConfigError> {
    if let Err(e) = validate_not_empty(&yaml_config) {
        return Err(e);
    }

    if let Err(e) = validate_service_references(&yaml_config) {
        return Err(e);
    }

    let mut services: HashMap<String, Service> = HashMap::new();
    let mut domains: HashMap<String, Domain> = HashMap::new();

    // Convert YamlServerService to Service
    for yaml_service in yaml_config.services {
        if let Err(e) = validate_url_origin(&yaml_service.location) {
            return Err(e);
        }

        let path_modifiers = match yaml_service.path_modifiers {
            Some(pm) => convert_path_modifiers(pm),
            None => Ok(Vec::new()),
        }?;

        let service = Service {
            origin: yaml_service.location,
            path_modifiers,
        };

        services.insert(yaml_service.name, service);
    }

    // Convert YamlDomain to Domain
    for yaml_domain in yaml_config.domains {
        let routes = match yaml_domain.routes {
            Some(dr) => convert_domain_routes(dr),
            None => Ok(Vec::new()),
        }?;

        let domain = Domain {
            default_service: yaml_domain.default_service,
            routes,
        };

        domains.insert(yaml_domain.domain, domain);
    }

    let domain_names = domains.keys().cloned().collect();

    Ok(ServerConfig {
        services,
        domains,
        domain_selection_order: choose_domain_ordering(domain_names),
    })
}

fn convert_path_modifiers(
    yaml_path_modifiers: Vec<YamlPathModifier>,
) -> Result<Vec<PathModifier>, ConfigError> {
    yaml_path_modifiers
        .into_iter()
        .map(|path_modifier| {
            let source = Regex::new(&path_modifier.source);
            match source {
                Err(e) => Err(ConfigError::InvalidRegex(path_modifier.source, e)),
                Ok(s) => Ok(PathModifier {
                    source: s,
                    target: path_modifier.target,
                }),
            }
        })
        .collect()
}

fn convert_domain_routes(yaml_routes: Vec<YamlRoute>) -> Result<Vec<Route>, ConfigError> {
    yaml_routes
        .into_iter()
        .map(|route| {
            let path = Regex::new(&route.path);
            match path {
                Err(e) => Err(ConfigError::InvalidRegex(route.path, e)),
                Ok(p) => Ok(Route {
                    path: p,
                    service: route.service,
                }),
            }
        })
        .collect()
}

fn validate_not_empty(server_config: &YamlServerConfig) -> Result<(), ConfigError> {
    if server_config.services.is_empty() {
        return Err(ConfigError::Empty);
    }
    if server_config.domains.is_empty() {
        return Err(ConfigError::Empty);
    }

    Ok(())
}

fn validate_service_references(server_config: &YamlServerConfig) -> Result<(), ConfigError> {
    let service_names: HashSet<&String> = server_config.services.iter().map(|s| &s.name).collect();

    for domain in &server_config.domains {
        if !service_names.contains(&domain.default_service) {
            return Err(ConfigError::NoSuchService(
                domain.default_service.to_string(),
            ));
        }

        if let Some(routes) = &domain.routes {
            for route in routes {
                if !service_names.contains(&route.service) {
                    return Err(ConfigError::NoSuchService(route.service.to_string()));
                }
            }
        }
    }

    Ok(())
}

fn validate_url_origin(url: &Url) -> Result<(), ConfigError> {
    let origin = url.origin();
    if !origin.is_tuple() {
        return Err(ConfigError::InvalidURL(url.to_string()));
    }

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ConfigError::InvalidURL(url.to_string()));
    }

    Ok(())
}

fn choose_domain_ordering(domains: Vec<String>) -> Vec<String> {
    let mut sorted_domains = domains;
    sorted_domains.sort_by(|a, b| {
        let a_subdomains: Vec<&str> = a.split('.').collect();
        let b_subdomains: Vec<&str> = b.split('.').collect();

        let a_len = a_subdomains.len();
        let b_len = b_subdomains.len();

        if a_len != b_len {
            b_len.cmp(&a_len)
        } else {
            a_subdomains
                .iter()
                .zip(b_subdomains.iter())
                .map(|(a_sub, b_sub)| b_sub.len().cmp(&a_sub.len()))
                .find(|&ord| ord != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        }
    });

    sorted_domains
}

pub fn server_config_to_yaml(server_config: ServerConfig) -> String {
    let services: Vec<YamlServerService> = server_config
        .services
        .into_iter()
        .map(|(name, service)| {
            let path_modifiers = if service.path_modifiers.len() == 0 {
                None
            } else {
                Some(
                    service
                        .path_modifiers
                        .into_iter()
                        .map(|path_modifier| YamlPathModifier {
                            source: path_modifier.source.to_string(),
                            target: path_modifier.target,
                        })
                        .collect(),
                )
            };

            YamlServerService {
                name,
                location: service.origin,
                path_modifiers,
            }
        })
        .collect();

    let domains: Vec<YamlDomain> = server_config
        .domains
        .into_iter()
        .map(|(domain, domain_data)| {
            let default_service = domain_data.default_service;
            let routes = if domain_data.routes.len() == 0 {
                None
            } else {
                Some(
                    domain_data
                        .routes
                        .into_iter()
                        .map(|route| YamlRoute {
                            path: route.path.to_string(),
                            service: route.service,
                        })
                        .collect(),
                )
            };

            YamlDomain {
                domain,
                default_service,
                routes,
            }
        })
        .collect();

    let yaml_server_config = YamlServerConfig { services, domains };

    // This should never fail, due to previous validation
    serde_yaml::to_string(&yaml_server_config).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF_STR: &str = r#"
    services:
      - name: frontend
        location: http://localhost:8000
        path_modifiers:
          - source: /foo/(.*)
            target: /bar/$1
      - name: backend
        location: http://localhost:8001/
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
    fn test_convert_server_config() {
        let input_str = String::from(CONF_STR);

        let server_config = new_server_config(input_str).unwrap();
        check_means_same_as_input_conf(&server_config);

        // Inverse should mean the same thing
        let output_conf = server_config_to_yaml(server_config);
        let second_server_conf = new_server_config(output_conf).unwrap();
        check_means_same_as_input_conf(&second_server_conf);
    }

    fn check_means_same_as_input_conf(server_config: &ServerConfig) {
        // Test services
        assert_eq!(server_config.services.len(), 2);
        assert!(server_config.services.contains_key("frontend"));
        assert!(server_config.services.contains_key("backend"));
        assert_eq!(
            server_config.services.get("frontend").unwrap().origin,
            Url::parse("http://localhost:8000").unwrap()
        );
        assert_eq!(
            server_config
                .services
                .get("frontend")
                .unwrap()
                .path_modifiers[0]
                .source
                .as_str(),
            "/foo/(.*)"
        );
        assert_eq!(
            server_config
                .services
                .get("frontend")
                .unwrap()
                .path_modifiers[0]
                .target,
            "/bar/$1"
        );
        assert_eq!(
            server_config.services.get("backend").unwrap().origin,
            Url::parse("http://localhost:8001").unwrap()
        );
        assert!(server_config
            .services
            .get("backend")
            .unwrap()
            .path_modifiers
            .is_empty());

        // Test domains
        assert_eq!(server_config.domains.len(), 2);
        assert!(server_config.domains.contains_key("example.com"));
        assert!(server_config.domains.contains_key("api.example.com"));
        assert_eq!(
            server_config
                .domains
                .get("example.com")
                .unwrap()
                .default_service,
            "frontend"
        );
        assert_eq!(
            server_config.domains.get("example.com").unwrap().routes[0]
                .path
                .as_str(),
            "/api/v1/.*"
        );
        assert_eq!(
            server_config.domains.get("example.com").unwrap().routes[0].service,
            "backend"
        );
        assert_eq!(
            server_config
                .domains
                .get("api.example.com")
                .unwrap()
                .default_service,
            "backend"
        );
        assert!(server_config
            .domains
            .get("api.example.com")
            .unwrap()
            .routes
            .is_empty());
    }

    #[test]
    fn test_choose_domain_ordering() {
        let input = vec![
            "example.com".to_string(),
            "api.example.com".to_string(),
            "render-api.example.com".to_string(),
            "another-example.com".to_string(),
        ];

        let expected_output = vec![
            "render-api.example.com".to_string(),
            "api.example.com".to_string(),
            "another-example.com".to_string(),
            "example.com".to_string(),
        ];

        assert_eq!(choose_domain_ordering(input), expected_output);
    }

    #[test]
    fn test_choose_domain_ordering_with_same_length() {
        let input = vec![
            "a.domain.com".to_string(),
            "b.domain.com".to_string(),
            "c.domain.com".to_string(),
        ];

        let expected_output = vec![
            "a.domain.com".to_string(),
            "b.domain.com".to_string(),
            "c.domain.com".to_string(),
        ];

        assert_eq!(choose_domain_ordering(input), expected_output);
    }
}
