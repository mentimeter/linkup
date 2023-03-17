use std::collections::{HashMap, HashSet};
use regex::Regex;
use thiserror::Error;
use url::{Host, Origin, Url};

mod yaml_server_config;

use crate::yaml_server_config::*;


pub struct ServerConfig {
    services: HashMap<String, Service>,
    domains: HashMap<String, Domain>,
}

pub struct Service {
    origin: String,
    path_modifiers: Vec<PathModifier>,
}

pub struct PathModifier {
    source: Regex,
    target: String,
}

pub struct Domain {
    default_service: String,
    routes: Vec<Route>,
}

pub struct Route {
    path: Regex,
    service: String,
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
    #[error("unknown error")]
    Unknown,
}


pub fn new_server_config(input_yaml_conf: String) -> Result<ServerConfig, ConfigError> {
    let yaml_config_res : Result<YamlServerConfig, serde_yaml::Error>= serde_yaml::from_str(&input_yaml_conf);
    match yaml_config_res {
        Err(e) => Err(ConfigError::Format(e)),
        Ok(c) => convert_server_config(c)
    }
}

fn convert_server_config(yaml_config: YamlServerConfig) -> Result<ServerConfig, ConfigError> {
    if let Err(e) = validate_not_empty(&yaml_config) {
        return Err(e)
    }

    if let Err(e) = validate_service_references(&yaml_config) {
        return Err(e)
    }

    let mut services: HashMap<String, Service> = HashMap::new();
    let mut domains: HashMap<String, Domain> = HashMap::new();

    // Convert YamlServerService to Service
    for yaml_service in yaml_config.services {
        if let Err(e) = validate_url_origin(&yaml_service.location) {
            return Err(e)
        }

        let path_modifiers = match yaml_service.path_modifiers {
            Some(pm) => convert_path_modifiers(pm),
            None => Ok(Vec::new()),
        }?;

        let mut origin = yaml_service.location.to_string();
        origin.pop(); // Assume no /
        let service = Service {
            origin,
            path_modifiers,
        };

        services.insert(yaml_service.name, service);
    }

    // Convert YamlDomain to Domain
    for yaml_domain in yaml_config.domains {
        let routes = match yaml_domain.routes {
            Some(dr) => convert_domain_routes(dr),
            None => Ok(Vec::new())
        }?;

        let domain = Domain {
            default_service: yaml_domain.default_service,
            routes,
        };

        domains.insert(yaml_domain.domain, domain);
    }

    Ok(ServerConfig {
        services,
        domains,
    })
}

fn convert_path_modifiers(yaml_path_modifiers: Vec<YamlPathModifier>) -> Result<Vec<PathModifier>, ConfigError> {
    yaml_path_modifiers
        .into_iter()
        .map(|path_modifier| {
            let source =  Regex::new(&path_modifier.source);
            match source {
                Err(e) => Err(ConfigError::InvalidRegex(path_modifier.source, e)),
                Ok(s) => Ok(PathModifier{
                    source: s,
                    target: path_modifier.target,
                })
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
                Ok(p) => Ok(Route{
                    path: p,
                    service: route.service,
                })
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
            return Err(ConfigError::NoSuchService(domain.default_service.to_string()));
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
        return Err(ConfigError::InvalidURL(url.to_string()))
    }

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(ConfigError::InvalidURL(url.to_string()))
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_server_config() {
        let config_str = String::from(r#"
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
        "#);

        let server_config = new_server_config(config_str).unwrap();

        // Test services
        assert_eq!(server_config.services.len(), 2);
        assert!(server_config.services.contains_key("frontend"));
        assert!(server_config.services.contains_key("backend"));
        assert_eq!(
            server_config.services.get("frontend").unwrap().origin,
            "http://localhost:8000"
        );
        assert_eq!(
            server_config.services.get("frontend").unwrap().path_modifiers[0].source.as_str(),
            "/foo/(.*)"
        );
        assert_eq!(
            server_config.services.get("frontend").unwrap().path_modifiers[0].target,
            "/bar/$1"
        );
        assert_eq!(
            server_config.services.get("backend").unwrap().origin,
            "http://localhost:8001"
        );
        assert!(server_config.services.get("backend").unwrap().path_modifiers.is_empty());

        // Test domains
        assert_eq!(server_config.domains.len(), 2);
        assert!(server_config.domains.contains_key("example.com"));
        assert!(server_config.domains.contains_key("api.example.com"));
        assert_eq!(
            server_config.domains.get("example.com").unwrap().default_service,
            "frontend"
        );
        assert_eq!(
            server_config.domains.get("example.com").unwrap().routes[0].path.as_str(),
            "/api/v1/.*"
        );
        assert_eq!(
            server_config.domains.get("example.com").unwrap().routes[0].service,
            "backend"
        );
        assert_eq!(
            server_config.domains.get("api.example.com").unwrap().default_service,
            "backend"
        );
        assert!(server_config.domains.get("api.example.com").unwrap().routes.is_empty());
    }
}