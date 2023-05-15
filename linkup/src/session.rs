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

#[derive(Clone, Debug)]
pub struct Domain {
    pub default_service: String,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug)]
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

impl TryFrom<StorableRewrite> for Rewrite {
    type Error = ConfigError;

    fn try_from(value: StorableRewrite) -> Result<Self, Self::Error> {
        let source: Result<Regex, regex::Error> = Regex::new(&value.source);
        match source {
            Err(e) => Err(ConfigError::InvalidRegex(value.source, e)),
            Ok(s) => Ok(Rewrite {
                source: s,
                target: value.target,
            }),
        }
    }
}

impl TryFrom<StorableRoute> for Route {
    type Error = ConfigError;

    fn try_from(value: StorableRoute) -> Result<Self, Self::Error> {
        let path = Regex::new(&value.path);
        match path {
            Err(e) => Err(ConfigError::InvalidRegex(value.path, e)),
            Ok(p) => Ok(Route {
                path: p,
                service: value.service,
            }),
        }
    }
}

impl TryFrom<StorableSession> for Session {
    type Error = ConfigError;

    fn try_from(value: StorableSession) -> Result<Self, Self::Error> {
        validate_not_empty(&value)?;
        validate_service_references(&value)?;

        let mut services: HashMap<String, Service> = HashMap::new();
        let mut domains: HashMap<String, Domain> = HashMap::new();

        for stored_service in value.services {
            validate_url_origin(&stored_service.location)?;

            let rewrites = match stored_service.rewrites {
                Some(pm) => pm.into_iter().map(|r| r.try_into()).collect(),
                None => Ok(Vec::new()),
            }?;

            let service = Service {
                origin: stored_service.location,
                rewrites,
            };

            services.insert(stored_service.name, service);
        }

        for stored_domain in value.domains {
            let routes = match stored_domain.routes {
                Some(dr) => dr.into_iter().map(|r| r.try_into()).collect(),
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
            session_token: value.session_token,
            services,
            domains,
            domain_selection_order: choose_domain_ordering(domain_names),
        })
    }
}

impl TryFrom<serde_json::Value> for Session {
    type Error = ConfigError;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        let session_yml_res: Result<StorableSession, serde_json::Error> =
            serde_json::from_value(value);

        match session_yml_res {
            Err(e) => Err(ConfigError::JsonFormat(e)),
            Ok(c) => c.try_into(),
        }
    }
}

impl From<Session> for StorableSession {
    fn from(value: Session) -> Self {
        let services: Vec<StorableService> = value
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

        let domains: Vec<StorableDomain> = value
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
            session_token: value.session_token,
            services,
            domains,
        }
    }
}

pub fn update_session_req_from_json(input_json: String) -> Result<(String, Session), ConfigError> {
    let update_session_req_res: Result<UpdateSessionRequest, serde_json::Error> =
        serde_json::from_str(&input_json);

    match update_session_req_res {
        Err(e) => Err(ConfigError::JsonFormat(e)),
        Ok(c) => {
            let server_conf = StorableSession {
                session_token: c.session_token,
                services: c.services,
                domains: c.domains,
            }
            .try_into();

            match server_conf {
                Err(e) => Err(e),
                Ok(sc) => Ok((c.desired_name, sc)),
            }
        }
    }
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

pub fn session_to_json(session: Session) -> String {
    let storable_session: StorableSession = session.into();

    // This should never fail, due to previous validation
    serde_json::to_string(&storable_session).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF_STR: &str = r#"
    {
        "session_token": "abcxyz",
        "services": [
            {
                "name": "frontend",
                "location": "http://localhost:8000",
                "rewrites": [
                    {
                        "source": "/foo/(.*)",
                        "target": "/bar/$1"
                    }
                ]
            },
            {
                "name": "backend",
                "location": "http://localhost:8001/"
            }
        ],
        "domains": [
            {
                "domain": "example.com",
                "default_service": "frontend",
                "routes": [
                    {
                        "path": "/api/v1/.*",
                        "service": "backend"
                    }
                ]
            },
            {
                "domain": "api.example.com",
                "default_service": "backend"
            }
        ]
    }
    "#;

    #[test]
    fn test_convert_server_config() {
        let input_str = String::from(CONF_STR);

        let server_config_value = serde_json::from_str::<serde_json::Value>(&input_str).unwrap();
        let server_config: Session = server_config_value.try_into().unwrap();
        check_means_same_as_input_conf(&server_config);

        // Inverse should mean the same thing
        let output_conf = session_to_json(server_config);
        let output_conf_value = serde_json::from_str::<serde_json::Value>(&output_conf).unwrap();
        let second_server_conf: Session = output_conf_value.try_into().unwrap();
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
            server_config.services.get("frontend").unwrap().rewrites[0]
                .source
                .as_str(),
            "/foo/(.*)"
        );
        assert_eq!(
            server_config.services.get("frontend").unwrap().rewrites[0].target,
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
