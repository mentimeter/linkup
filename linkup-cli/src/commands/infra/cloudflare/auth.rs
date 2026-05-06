use reqwest::header::HeaderMap;

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
