use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};
use thiserror::Error;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone)]
pub struct Session {
    pub session_token: String,
    pub services: HashMap<String, Service>,
    pub domains: HashMap<String, Domain>,
    pub domain_selection_order: Vec<String>,
}

#[derive(Clone)]
pub struct Service {
    pub origin: Url,
    pub rewrites: Vec<Rewrite>,
}

#[derive(Clone)]
pub struct Rewrite {
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
pub struct UpdateSessionRequest {
    pub desired_name: String,
    pub session_token: String,
    pub services: Vec<StorableService>,
    pub domains: Vec<StorableDomain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StorableSession {
    pub session_token: String,
    pub services: Vec<StorableService>,
    pub domains: Vec<StorableDomain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StorableService {
    pub name: String,
    pub location: Url,
    pub rewrites: Option<Vec<StorableRewrite>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorableRewrite {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorableDomain {
    pub domain: String,
    pub default_service: String,
    pub routes: Option<Vec<StorableRoute>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorableRoute {
    pub path: String,
    pub service: String,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("linkup session yml format error: {0}")]
    YmlFormat(#[from] serde_yaml::Error),
    #[error("linkup session json format error: {0}")]
    JsonFormat(#[from] serde_json::Error),
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

pub fn session_from_json(input_json: String) -> Result<Session, ConfigError> {
    let session_yml_res: Result<StorableSession, serde_json::Error> =
        serde_json::from_str(&input_json);
    match session_yml_res {
        Err(e) => Err(ConfigError::JsonFormat(e)),
        Ok(c) => convert_stored_session(c),
    }
}

pub fn update_session_req_from_json(
    input_json: String,
) -> Result<(String, Session), ConfigError> {
    let update_session_req_res: Result<UpdateSessionRequest, serde_json::Error> =
        serde_json::from_str(&input_json);
    match update_session_req_res {
        Err(e) => Err(ConfigError::JsonFormat(e)),
        Ok(c) => {
            let server_conf = convert_stored_session(StorableSession {
                session_token: c.session_token,
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

pub fn session_from_yml(input_yaml: String) -> Result<Session, ConfigError> {
    let session_yml_res: Result<StorableSession, serde_yaml::Error> =
        serde_yaml::from_str(&input_yaml);
    match session_yml_res {
        Err(e) => Err(ConfigError::YmlFormat(e)),
        Ok(c) => convert_stored_session(c),
    }
}

pub fn update_session_req_from_yml(
    input_yaml: String,
) -> Result<(String, Session), ConfigError> {
    let update_session_req_res: Result<UpdateSessionRequest, serde_yaml::Error> =
        serde_yaml::from_str(&input_yaml);
    match update_session_req_res {
        Err(e) => Err(ConfigError::YmlFormat(e)),
        Ok(c) => {
            let server_conf = convert_stored_session(StorableSession {
                session_token: c.session_token,
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

fn convert_stored_session(stored_session: StorableSession) -> Result<Session, ConfigError> {
    validate_not_empty(&stored_session)?;
    validate_service_references(&stored_session)?;

    let mut services: HashMap<String, Service> = HashMap::new();
    let mut domains: HashMap<String, Domain> = HashMap::new();

    for stored_service in stored_session.services {
        validate_url_origin(&stored_service.location)?;

        let rewrites = match stored_service.rewrites {
            Some(pm) => convert_rewrites(pm),
            None => Ok(Vec::new()),
        }?;

        let service = Service {
            origin: stored_service.location,
            rewrites,
        };

        services.insert(stored_service.name, service);
    }

    for stored_domain in stored_session.domains {
        let routes = match stored_domain.routes {
            Some(dr) => convert_domain_routes(dr),
            None => Ok(Vec::new()),
        }?;

        let domain = Domain {
            default_service: stored_domain.default_service,
            routes,
        };

        domains.insert(stored_domain.domain, domain);
    }

    let domain_names = domains.keys().cloned().collect();

    Ok(Session {
        session_token: stored_session.session_token,
        services,
        domains,
        domain_selection_order: choose_domain_ordering(domain_names),
    })
}

fn convert_rewrites(
    stored_rewrites: Vec<StorableRewrite>,
) -> Result<Vec<Rewrite>, ConfigError> {
    stored_rewrites
        .into_iter()
        .map(|path_modifier| {
            let source = Regex::new(&path_modifier.source);
            match source {
                Err(e) => Err(ConfigError::InvalidRegex(path_modifier.source, e)),
                Ok(s) => Ok(Rewrite {
                    source: s,
                    target: path_modifier.target,
                }),
            }
        })
        .collect()
}

fn convert_domain_routes(stored_routes: Vec<StorableRoute>) -> Result<Vec<Route>, ConfigError> {
    stored_routes
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

fn validate_not_empty(server_config: &StorableSession) -> Result<(), ConfigError> {
    if server_config.services.is_empty() {
        return Err(ConfigError::Empty);
    }
    if server_config.domains.is_empty() {
        return Err(ConfigError::Empty);
    }

    Ok(())
}

fn validate_service_references(server_config: &StorableSession) -> Result<(), ConfigError> {
    let service_names: HashSet<&str> = server_config
        .services
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    for domain in &server_config.domains {
        if !service_names.contains(&domain.default_service.as_str()) {
            return Err(ConfigError::NoSuchService(
                domain.default_service.to_string(),
            ));
        }

        if let Some(routes) = &domain.routes {
            for route in routes {
                if !service_names.contains(&route.service.as_str()) {
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

fn session_to_storable(session: Session) -> StorableSession {
    let services: Vec<StorableService> = session
        .services
        .into_iter()
        .map(|(name, service)| {
            let rewrites = if service.rewrites.is_empty() {
                None
            } else {
                Some(
                    service
                        .rewrites
                        .into_iter()
                        .map(|path_modifier| StorableRewrite {
                            source: path_modifier.source.to_string(),
                            target: path_modifier.target,
                        })
                        .collect(),
                )
            };

            StorableService {
                name,
                location: service.origin,
                rewrites,
            }
        })
        .collect();

    let domains: Vec<StorableDomain> = session
        .domains
        .into_iter()
        .map(|(domain, domain_data)| {
            let default_service = domain_data.default_service;
            let routes = if domain_data.routes.is_empty() {
                None
            } else {
                Some(
                    domain_data
                        .routes
                        .into_iter()
                        .map(|route| StorableRoute {
                            path: route.path.to_string(),
                            service: route.service,
                        })
                        .collect(),
                )
            };

            StorableDomain {
                domain,
                default_service,
                routes,
            }
        })
        .collect();

    StorableSession {
        session_token: session.session_token,
        services,
        domains,
    }
}

pub fn session_to_yml(session: Session) -> String {
    let storable_session = session_to_storable(session);

    // This should never fail, due to previous validation
    serde_yaml::to_string(&storable_session).unwrap()
}

pub fn session_to_json(session: Session) -> String {
    let storable_session = session_to_storable(session);

    // This should never fail, due to previous validation
    serde_json::to_string(&storable_session).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF_STR: &str = r#"
    session_token: abcxyz
    services:
      - name: frontend
        location: http://localhost:8000
        rewrites:
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

        let server_config = session_from_yml(input_str).unwrap();
        check_means_same_as_input_conf(&server_config);

        // Inverse should mean the same thing
        let output_conf = session_to_yml(server_config);
        let second_server_conf = session_from_yml(output_conf).unwrap();
        check_means_same_as_input_conf(&second_server_conf);
    }

    fn check_means_same_as_input_conf(server_config: &Session) {
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
                .rewrites[0]
                .source
                .as_str(),
            "/foo/(.*)"
        );
        assert_eq!(
            server_config
                .services
                .get("frontend")
                .unwrap()
                .rewrites[0]
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
            .rewrites
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
