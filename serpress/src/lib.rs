use thiserror::Error;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    services: Vec<ServiceChosen>,
    domains: Vec<Domain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LocalConfig {
    serpress: SerpressConfig,
    services: Vec<ServiceConfig>,
    domains: Vec<Domain>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SerpressConfig {
    remote: Url,
    local: Url,

    name_kind: NameKind,    
    alive_time: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceChosen {
    name: String,
    location: Url,
    path_modifiers: Option<Vec<PathModifier>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceConfig {
    name: String,
    remote: Url,
    local: Url,
    directory: Option<String>,
    path_modifiers: Option<Vec<PathModifier>>,
}


#[derive(Debug, Deserialize, Serialize)]
pub struct PathModifier {
    source: String,
    target: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Domain {
    domain: String,
    default_service: Option<String>,
    routes: Option<Vec<Route>>
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Route {
    path: String,
    service: String,
}


#[derive(Debug, Deserialize, Serialize)]
enum NameKind {
    Animal,
    SixChar,
}

pub fn new_server_config(input_conf: String) -> Result<ServerConfig, ConfigError> {
    let config : Result<ServerConfig, serde_yaml::Error>= serde_yaml::from_str(&input_conf);

    match config {
        Err(e) => Err(ConfigError::Format(e)),
        Ok(c) => {
            if let Err(e) = check_config_domains(&c) {
                return Err(e)
            }

            if let Err(e) = check_domain_services_valid(&c) {
                return Err(e)
            }

            Ok(c)
        }
    }
}

pub fn get_service(conf: &ServerConfig, name: String) -> Result<&ServiceChosen, ConfigError> {
    for s in conf.services.iter() {
        if s.name == name {
            return Ok(s)
        }
    }
    Err(ConfigError::NoSuchService(name))
}

fn check_config_domains(conf: &ServerConfig) -> Result<(), ConfigError> {
    for domain in conf.domains.iter()  {
        match (&domain.default_service, &domain.routes) {
            (None, None) => return Err(ConfigError::DomainError),
            _ => (),
        }
    }
    Ok(())
}

fn check_domain_services_valid(conf: &ServerConfig) -> Result<(), ConfigError> {
    println!("domains: {:#?}", conf.domains);
    println!("services: {:#?}", conf.services);
    for domain in conf.domains.iter()  {
        if let Some(s) = &domain.default_service {
            match get_service(conf, s.to_string()) {
            Err(e) => return Err(e),
                Ok(_) => () 
            }
        }

        if let Some(rs) = &domain.routes {
            for r in rs.iter() {
                match get_service(conf, r.service.to_string()) {
                Err(e) => return Err(e),
                    Ok(_) => () 
                }
            }
        }
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("serpress config format error: {0}")]
    Format(#[from] serde_yaml::Error),
    #[error("no such service: {0}")]
    NoSuchService(String),
    #[error("domain config error")]
    DomainError,
    #[error("unknown error")]
    Unknown,
}


#[cfg(test)]
mod tests {
    use crate::new_server_config;

    #[test]
    fn accepts_default_valid_server_config() {
        let server_conf_res = new_server_config(String::from(r#"
        services:
            - name: core
              location: http://remote-server.dev
        domains:
            - domain: api.serpress.dev
              default_service: core
        "#));

        let server_conf = server_conf_res.unwrap();

        assert_eq!(server_conf.domains[0].domain, String::from("api.serpress.dev"));
        assert_eq!(server_conf.services[0].name, String::from("core"));
    }

    #[test]
    fn fails_server_empty_conf() {
        let server_conf_res = new_server_config(String::from(r#"
        services:
            - name: core
              location: http://remote-server.dev
        "#));

        assert!(server_conf_res.is_err(), "needs domains should fail")
    }

    #[test]
    fn fails_server_config_no_service_or_routes() {
        let server_conf_res = new_server_config(String::from(r#"
        services:
            - name: core
              location: http://remote-server.dev
        domains:
            - domain: api.serpress.dev
        "#));

        assert!(server_conf_res.is_err(), "config without default service or routes should fail")
    }

    #[test]
    fn fails_server_config_no_such_service() {
        let server_conf = new_server_config(String::from(r#"
        services:
            - name: core
              location: http://remote-server.dev
        domains:
            - domain: api.serpress.dev
              default_service: www
        "#));

        assert!(server_conf.is_err(), "no such service should fail");

        let server_conf = new_server_config(String::from(r#"
        services:
            - name: core
              location: http://remote-server.dev
        domains:
            - domain: api.serpress.dev
              routes:
                - path: /*
                  service: www
        "#));

        assert!(server_conf.is_err(), "no such service should fail")
    }
}