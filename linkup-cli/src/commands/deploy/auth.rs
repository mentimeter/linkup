use std::env;

use reqwest::header::HeaderMap;

use super::DeployError;

pub trait CloudflareApiAuth {
    fn headers(&self) -> HeaderMap;
}

pub struct CloudflareGlobalTokenAuth {
    api_key: String,
    email: String,
}

impl CloudflareGlobalTokenAuth {
    pub fn new(api_key: String, email: String) -> Self {
        Self { api_key, email }
    }
}

impl CloudflareApiAuth for CloudflareGlobalTokenAuth {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Auth-Email",
            reqwest::header::HeaderValue::from_str(&self.email).unwrap(),
        );
        headers.insert(
            "X-Auth-Key",
            reqwest::header::HeaderValue::from_str(&self.api_key).unwrap(),
        );
        headers
    }
}

pub struct CloudflareTokenAuth {
    api_key: String,
}

impl CloudflareTokenAuth {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl CloudflareApiAuth for CloudflareTokenAuth {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.api_key)).unwrap(),
        );
        headers
    }
}

pub fn get_auth() -> Result<Box<dyn CloudflareApiAuth>, DeployError> {
    let api_key = env::var("CLOUDFLARE_API_KEY");
    let email = env::var("CLOUDFLARE_EMAIL");
    let api_token = env::var("CLOUDFLARE_API_TOKEN");

    match (api_key, email, api_token) {
        (Ok(api_key), Ok(email), _) => Ok(Box::new(CloudflareGlobalTokenAuth::new(api_key, email))),
        (_, _, Ok(api_token)) => Ok(Box::new(CloudflareTokenAuth::new(api_token))),
        _ => Err(DeployError::NoAuthenticationError),
    }
}
