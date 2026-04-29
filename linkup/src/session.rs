use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{NameKind, TunnelData, config::Config};

pub const PREVIEW_SESSION_TOKEN: &str = "preview_session";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Domain {
    pub domain: String,
    pub default_service: String,
    pub routes: Option<Vec<Route>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Route {
    #[serde(
        serialize_with = "crate::serde_ext::serialize_regex",
        deserialize_with = "crate::serde_ext::deserialize_regex"
    )]
    pub path: Regex,
    pub service: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum UpsertSessionRequest {
    Named {
        desired_name: String,
        session_token: String,
        services: Vec<SessionService>,
        domains: Vec<Domain>,
        #[serde(
            default,
            serialize_with = "crate::serde_ext::serialize_opt_vec_regex",
            deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex"
        )]
        cache_routes: Option<Vec<Regex>>,
    },
    Unnamed {
        name_kind: NameKind,
        session_token: Option<String>,
        services: Vec<SessionService>,
        domains: Vec<Domain>,
        #[serde(
            default,
            serialize_with = "crate::serde_ext::serialize_opt_vec_regex",
            deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex"
        )]
        cache_routes: Option<Vec<Regex>>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionResponse {
    pub session_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TunneledSessionResponse {
    pub session_name: String,
    pub tunnel_data: TunnelData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub session_token: String,
    pub services: Vec<SessionService>,
    pub domains: Vec<Domain>,
    #[serde(
        default,
        serialize_with = "crate::serde_ext::serialize_opt_vec_regex",
        deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex"
    )]
    pub cache_routes: Option<Vec<Regex>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionService {
    pub name: String,
    pub location: Url,
    pub rewrites: Option<Vec<Rewrite>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rewrite {
    #[serde(
        serialize_with = "crate::serde_ext::serialize_regex",
        deserialize_with = "crate::serde_ext::deserialize_regex"
    )]
    pub source: Regex,
    pub target: String,
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

impl Session {
    pub fn new(
        session_token: String,
        services: Vec<SessionService>,
        domains: Vec<Domain>,
        cache_routes: Option<Vec<Regex>>,
    ) -> Result<Self, ConfigError> {
        let session = Self {
            session_token,
            services,
            domains,
            cache_routes,
        };

        session.validate()?;

        Ok(session)
    }

    pub fn get_service(&self, service_name: &str) -> Option<&SessionService> {
        self.services
            .iter()
            .find(|service| service.name == service_name)
    }

    pub fn get_domain(&self, domain: &str) -> Option<&Domain> {
        self.domains
            .iter()
            .find(|domain_record| domain_record.domain == domain)
    }

    pub fn sha(&self) -> String {
        let mut sorted_self = self.clone();
        sorted_self.services.sort_by(|a, b| a.name.cmp(&b.name));
        sorted_self.domains.sort_by(|a, b| a.domain.cmp(&b.domain));

        if let Some(cache_routes) = sorted_self.cache_routes.as_mut() {
            cache_routes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        }

        let serialized_self =
            serde_json::to_string(&sorted_self).expect("Session should be serializable");

        let mut hasher = Sha256::new();

        hasher.update(serialized_self);

        let result = hasher.finalize();
        hex::encode(result)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.services.is_empty() {
            return Err(ConfigError::Empty);
        }

        if self.domains.is_empty() {
            return Err(ConfigError::Empty);
        }

        let mut service_names: HashSet<&str> = HashSet::new();

        for service in &self.services {
            let url = &service.location;
            let origin = &url.origin();

            if !origin.is_tuple() {
                return Err(ConfigError::InvalidURL(url.to_string()));
            }

            if url.scheme() != "http" && url.scheme() != "https" {
                return Err(ConfigError::InvalidURL(url.to_string()));
            }

            service_names.insert(&service.name);
        }

        for domain in &self.domains {
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
}

impl TryFrom<UpsertSessionRequest> for Session {
    type Error = ConfigError;

    fn try_from(req: UpsertSessionRequest) -> Result<Self, Self::Error> {
        let (session_token, services, domains, cache_routes) = match req {
            UpsertSessionRequest::Named {
                services,
                domains,
                cache_routes,
                session_token,
                ..
            } => (session_token, services, domains, cache_routes),
            UpsertSessionRequest::Unnamed {
                services,
                domains,
                cache_routes,
                session_token,
                ..
            } => (
                session_token.unwrap_or_else(|| PREVIEW_SESSION_TOKEN.to_string()),
                services,
                domains,
                cache_routes,
            ),
        };

        let session = Self::new(session_token, services, domains, cache_routes)?;

        Ok(session)
    }
}

impl TryFrom<serde_json::Value> for Session {
    type Error = ConfigError;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        let session: Session = serde_json::from_value(value)?;

        session.validate()?;

        Ok(session)
    }
}

pub fn create_preview_req_from_config(
    config: &Config,
    services_overwrite: &[(String, Url)],
) -> UpsertSessionRequest {
    let mut session_services: Vec<SessionService> = Vec::with_capacity(config.services.len());

    for service in &config.services {
        let service_overwrite = services_overwrite
            .iter()
            .find(|overwrite| overwrite.0 == service.name);

        let location = match service_overwrite {
            Some((_, location_overwrite)) => location_overwrite.clone(),
            None => service.remote.clone(),
        };

        session_services.push(SessionService {
            name: service.name.clone(),
            location,
            rewrites: service.rewrites.clone(),
        });
    }

    UpsertSessionRequest::Unnamed {
        name_kind: NameKind::SixChar,
        session_token: None,
        services: session_services,
        domains: config.domains.clone(),
        cache_routes: config.linkup.cache_routes.clone(),
    }
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
        ],
        "cache_routes": [
            "/static/.*"
        ]
    }
    "#;

    #[test]
    fn test_convert_session() {
        let input_str = String::from(CONF_STR);

        let session_value = serde_json::from_str::<serde_json::Value>(&input_str).unwrap();
        let session: Session = session_value.try_into().unwrap();
        check_means_same_as_input_conf(&session);

        // Inverse should mean the same thing
        let output_session = serde_json::to_string(&session).unwrap();
        let output_session_value =
            serde_json::from_str::<serde_json::Value>(&output_session).unwrap();
        let second_session: Session = output_session_value.try_into().unwrap();
        check_means_same_as_input_conf(&second_session);
    }

    fn check_means_same_as_input_conf(session: &Session) {
        // Test services
        assert_eq!(session.services.len(), 2);

        let frontend_service = session.get_service("frontend").unwrap();
        assert_eq!(
            frontend_service.location,
            Url::parse("http://localhost:8000").unwrap()
        );

        assert_eq!(
            Some(1),
            frontend_service
                .rewrites
                .as_ref()
                .map(|rewrites| rewrites.len())
        );

        let frontend_service_rewrite = &frontend_service.rewrites.as_ref().unwrap()[0];
        assert_eq!(frontend_service_rewrite.source.as_str(), "/foo/(.*)");
        assert_eq!(frontend_service_rewrite.target, "/bar/$1");

        let backend_service = session.get_service("backend").unwrap();
        assert_eq!(
            backend_service.location,
            Url::parse("http://localhost:8001").unwrap()
        );
        assert!(backend_service.rewrites.is_none());

        // Test domains
        assert_eq!(2, session.domains.len());

        let example_domain = session.get_domain("example.com").unwrap();
        assert_eq!(example_domain.default_service, "frontend");

        assert_eq!(
            Some(1),
            example_domain.routes.as_ref().map(|routes| routes.len())
        );

        let example_domain_route = &example_domain.routes.as_ref().unwrap()[0];
        assert_eq!(example_domain_route.path.as_str(), "/api/v1/.*");
        assert_eq!(example_domain_route.service, "backend");

        let api_domain = session.get_domain("api.example.com").unwrap();
        assert_eq!(api_domain.default_service, "backend");
        assert!(api_domain.routes.is_none());

        // Test cache routes

        assert_eq!(session.cache_routes.as_ref().unwrap().len(), 1);
        assert_eq!(
            session.cache_routes.as_ref().unwrap()[0].as_str(),
            "/static/.*"
        );
    }
}
