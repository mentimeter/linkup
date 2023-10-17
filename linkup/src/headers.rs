use std::collections::HashMap;

use unicase::UniCase;

pub struct HeaderMap(HashMap<UniCase<String>, String>);

impl IntoIterator for &HeaderMap {
    type Item = (UniCase<String>, String);
    type IntoIter = std::collections::hash_map::IntoIter<UniCase<String>, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.clone().into_iter()
    }
}

impl Default for HeaderMap {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<HeaderMap> for HeaderMap {
    fn as_ref(&self) -> &HeaderMap {
        self
    }
}

impl HeaderMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(&UniCase::new(key.to_string()))
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(&UniCase::new(key.to_string()))
    }

    pub fn insert(&mut self, key: &str, value: impl ToString) -> Option<String> {
        self.0
            .insert(UniCase::new(key.to_string()), value.to_string())
    }

    pub fn extend(&mut self, iter: &HeaderMap) {
        self.0.extend(iter)
    }

    #[cfg(feature = "actix")]
    pub fn from_actix_request(req: &actix_web::HttpRequest) -> Self {
        req.headers().into()
    }

    #[cfg(feature = "worker")]
    pub fn from_worker_request(req: &worker::Request) -> Self {
        req.headers().into()
    }
}

#[cfg(feature = "reqwest")]
impl From<HeaderMap> for reqwest::header::HeaderMap {
    fn from(value: HeaderMap) -> Self {
        let mut header_map = reqwest::header::HeaderMap::new();

        for (key, value) in value.into_iter() {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&value) {
                    header_map.insert(header_name, header_value);
                }
            }
        }

        header_map
    }
}

#[cfg(feature = "worker")]
impl From<&worker::Headers> for HeaderMap {
    fn from(value: &worker::Headers) -> Self {
        value.into_iter().collect::<HeaderMap>()
    }
}

#[cfg(feature = "worker")]
impl FromIterator<(String, String)> for HeaderMap {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut headers = HeaderMap::new();
        for (k, v) in iter {
            headers.insert(k.as_str(), v);
        }

        headers
    }
}

#[cfg(feature = "actix")]
impl From<&actix_http::header::HeaderMap> for HeaderMap {
    fn from(value: &actix_http::header::HeaderMap) -> Self {
        value.into_iter().collect::<HeaderMap>()
    }
}

#[cfg(feature = "actix")]
impl<'a>
    FromIterator<(
        &'a actix_http::header::HeaderName,
        &'a actix_http::header::HeaderValue,
    )> for HeaderMap
{
    fn from_iter<
        T: IntoIterator<
            Item = (
                &'a actix_http::header::HeaderName,
                &'a actix_http::header::HeaderValue,
            ),
        >,
    >(
        iter: T,
    ) -> Self {
        let mut headers = HeaderMap::new();
        for (k, v) in iter {
            headers.insert(k.as_str(), v.to_str().unwrap_or(""));
        }

        headers
    }
}
