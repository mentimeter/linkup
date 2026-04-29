use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Domain, Rewrite};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub linkup: LinkupConfig,
    pub services: Vec<ServiceConfig>,
    pub domains: Vec<Domain>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LinkupConfig {
    pub worker_url: Url,
    pub worker_token: String,
    #[serde(
        default,
        deserialize_with = "crate::serde_ext::deserialize_opt_vec_regex",
        serialize_with = "crate::serde_ext::serialize_opt_vec_regex"
    )]
    pub cache_routes: Option<Vec<Regex>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServiceConfig {
    pub name: String,
    pub remote: Url,
    pub local: Url,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_ext::deserialize_optional_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub rewrites: Option<Vec<Rewrite>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HealthConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_ext::deserialize_optional_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub statuses: Option<Vec<u16>>,
}
