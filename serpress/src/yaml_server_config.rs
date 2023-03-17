use std::collections::HashSet;

use thiserror::Error;
use regex::Regex;

use serde::{Deserialize, Serialize};
use url::Url;

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

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlPathModifier {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlDomain {
    pub domain: String,
    pub default_service: String,
    pub routes: Option<Vec<YamlRoute>>
}

#[derive(Debug, Deserialize, Serialize)]
pub struct YamlRoute {
    pub path: String,
    pub service: String,
}

// #[derive(Debug, Deserialize, Serialize)]
// struct LocalConfig {
//     serpress: SerpressConfig,
//     services: Vec<ServiceConfig>,
//     domains: Vec<Domain>,
// }

// #[derive(Debug, Deserialize, Serialize)]
// struct SerpressConfig {
//     remote: Url,
//     local: Url,

//     name_kind: NameKind,    
//     alive_time: String,
// }

// #[derive(Debug, Deserialize, Serialize)]
// struct ServiceConfig {
//     name: String,
//     remote: Url,
//     local: Url,
//     directory: Option<String>,
//     path_modifiers: Option<Vec<PathModifier>>,
// }

// #[derive(Debug, Deserialize, Serialize)]
// enum NameKind {
//     Animal,
//     SixChar,
// }
