use std::collections::HashSet;
use thiserror::Error;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

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
pub struct UpdateSessionRequest {
    pub desired_name: String,
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
pub struct CreatePreviewRequest {
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
}

impl TryFrom<UpdateSessionRequest> for Session {
    type Error = ConfigError;

    fn try_from(req: UpdateSessionRequest) -> Result<Self, Self::Error> {
        let session = Self {
            session_token: req.session_token,
            services: req.services,
            domains: req.domains,
            cache_routes: req.cache_routes,
        };

        validate_not_empty(&session)?;
        validate_services(&session)?;

        Ok(session)
    }
}

impl TryFrom<CreatePreviewRequest> for Session {
    type Error = ConfigError;

    fn try_from(req: CreatePreviewRequest) -> Result<Self, Self::Error> {
        let session = Self {
            session_token: PREVIEW_SESSION_TOKEN.to_string(),
            services: req.services,
            domains: req.domains,
            cache_routes: req.cache_routes,
        };

        validate_not_empty(&session)?;
        validate_services(&session)?;

        Ok(session)
    }
}

impl TryFrom<serde_json::Value> for Session {
    type Error = ConfigError;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        let session = serde_json::from_value(value)?;

        validate_not_empty(&session)?;
        validate_services(&session)?;

        Ok(session)
    }
}

pub fn update_session_req_from_json(input_json: String) -> Result<(String, Session), ConfigError> {
    let update_session_req_res: UpdateSessionRequest = serde_json::from_str(&input_json)?;

    let session = Session {
        session_token: update_session_req_res.session_token,
        services: update_session_req_res.services,
        domains: update_session_req_res.domains,
        cache_routes: update_session_req_res.cache_routes,
    };

    Ok((update_session_req_res.desired_name, session))
}

pub fn create_preview_req_from_json(input_json: String) -> Result<Session, ConfigError> {
    let update_session_req_res: CreatePreviewRequest = serde_json::from_str(&input_json)?;

    let session = Session {
        session_token: String::from(PREVIEW_SESSION_TOKEN),
        services: update_session_req_res.services,
        domains: update_session_req_res.domains,
        cache_routes: None,
    };

    Ok(session)
}

fn validate_not_empty(server_config: &Session) -> Result<(), ConfigError> {
    if server_config.services.is_empty() {
        return Err(ConfigError::Empty);
    }
    if server_config.domains.is_empty() {
        return Err(ConfigError::Empty);
    }

    Ok(())
}

fn validate_services(server_config: &Session) -> Result<(), ConfigError> {
    let mut service_names: HashSet<&str> = HashSet::new();

    for service in &server_config.services {
        validate_url_origin(&service.location)?;

        service_names.insert(&service.name);
    }

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
    fn test_convert_server_config() {
        let input_str = String::from(CONF_STR);

        let server_config_value = serde_json::from_str::<serde_json::Value>(&input_str).unwrap();
        let server_config: Session = server_config_value.try_into().unwrap();
        check_means_same_as_input_conf(&server_config);

        // Inverse should mean the same thing
        let output_conf = serde_json::to_string(&server_config).unwrap();
        let output_conf_value = serde_json::from_str::<serde_json::Value>(&output_conf).unwrap();
        let second_server_conf: Session = output_conf_value.try_into().unwrap();
        check_means_same_as_input_conf(&second_server_conf);
    }

    fn check_means_same_as_input_conf(server_config: &Session) {
        // Test services
        assert_eq!(server_config.services.len(), 2);

        let frontend_service = server_config.get_service("frontend").unwrap();
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

        let backend_service = server_config.get_service("backend").unwrap();
        assert_eq!(
            backend_service.location,
            Url::parse("http://localhost:8001").unwrap()
        );
        assert!(backend_service.rewrites.is_none());

        // Test domains
        assert_eq!(2, server_config.domains.len());

        let example_domain = server_config.get_domain("example.com").unwrap();
        assert_eq!(example_domain.default_service, "frontend");

        assert_eq!(
            Some(1),
            example_domain.routes.as_ref().map(|routes| routes.len())
        );

        let example_domain_route = &example_domain.routes.as_ref().unwrap()[0];
        assert_eq!(example_domain_route.path.as_str(), "/api/v1/.*");
        assert_eq!(example_domain_route.service, "backend");

        let api_domain = server_config.get_domain("api.example.com").unwrap();
        assert_eq!(api_domain.default_service, "backend");
        assert!(api_domain.routes.is_none());

        // Test cache routes

        assert_eq!(server_config.cache_routes.as_ref().unwrap().len(), 1);
        assert_eq!(
            server_config.cache_routes.as_ref().unwrap()[0].as_str(),
            "/static/.*"
        );
    }
}
