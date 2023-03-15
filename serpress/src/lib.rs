use std::collections::HashMap;

use serde::Deserialize;
use url::{Url, ParseError};

#[derive(Deserialize)]
pub struct ServerConfig {
    serpress: Option<SerpressConfig>,
    services: HashMap<String, Service>,
    domains: HashMap<String, Domain>,
}

#[derive(Deserialize)]
pub struct LocalConfig {
    serpress: Option<SerpressConfig>,
    serpress_servers: Servers,
    services: HashMap<String, Service>,
    domains: HashMap<String, Domain>,
}

#[derive(Deserialize)]
struct SerpressConfig {
    name_kind: NameKind,    
    // Could be a toml_datetime
    alive_time: String,
}

#[derive(Deserialize)]
struct Servers {
    remote: Url,
    local: Url,
}

#[derive(Deserialize)]
struct Service {
    name: String,
}

#[derive(Deserialize)]
struct Domain {
    domain: String,
}

#[derive(Deserialize)]
enum NameKind {
    Animal,
    SixChar,
}

pub fn new_server_config(config: String) -> Result<ServerConfig, toml::de::Error> {
    toml::from_str(config.as_str())
}


#[cfg(test)]
mod tests {
    use crate::ServerConfig;

    #[test]
    fn accepts_default_valid_server_config() {
        let server_conf: ServerConfig = toml::from_str(r#"
        [services]

        [services.core]
        name = 'core'

        [domains]
        [domains.serpress]
        domain = 'serpress.dev' 
        "#).unwrap();
        assert_eq!(server_conf.domains.get("serpress").unwrap().domain, String::from("serpress.dev"));
        assert_eq!(server_conf.services.get("core").unwrap().name, String::from("core"));
    }
}