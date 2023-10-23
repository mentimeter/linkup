use std::{collections::HashMap, fmt};

use unicase::UniCase;

pub struct HeaderMap(HashMap<UniCase<String>, String>);

pub enum HeaderName {
    ForwardedHost,
    TraceParent,
    TraceState,
    LinkupDestination,
    Referer,
    Origin,
    Host,
}

impl From<HeaderName> for UniCase<String> {
    fn from(value: HeaderName) -> Self {
        match value {
            HeaderName::ForwardedHost => "x-forwarded-host".into(),
            HeaderName::TraceParent => "traceparent".into(),
            HeaderName::TraceState => "tracestate".into(),
            HeaderName::LinkupDestination => "linkup-destination".into(),
            HeaderName::Referer => "referer".into(),
            HeaderName::Origin => "origin".into(),
            HeaderName::Host => "host".into(),
        }
    }
}

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

impl HeaderMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn contains_key(&self, key: impl Into<UniCase<String>>) -> bool {
        self.0.contains_key(&key.into())
    }

    pub fn get(&self, key: impl Into<UniCase<String>>) -> Option<&str> {
        self.0.get(&key.into()).map(String::as_ref)
    }

    pub fn get_or_default<'a>(
        &'a self,
        key: impl Into<UniCase<String>>,
        default: &'a str,
    ) -> &'a str {
        match self.get(key) {
            Some(value) => value,
            None => default,
        }
    }

    pub fn insert(
        &mut self,
        key: impl Into<UniCase<String>>,
        value: impl ToString,
    ) -> Option<String> {
        self.0.insert(key.into(), value.to_string())
    }

    pub fn extend(&mut self, iter: &HeaderMap) {
        self.0.extend(iter)
    }

    pub fn remove(&mut self, key: impl Into<UniCase<String>>) -> Option<String> {
        self.0.remove(&key.into())
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

impl fmt::Debug for HeaderMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(k, v)| (k.as_ref(), v.as_str())))
            .finish()
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
impl From<&actix_web::http::header::HeaderMap> for HeaderMap {
    fn from(value: &actix_web::http::header::HeaderMap) -> Self {
        value.into_iter().collect::<HeaderMap>()
    }
}

#[cfg(feature = "actix")]
impl<'a>
    FromIterator<(
        &'a actix_web::http::header::HeaderName,
        &'a actix_web::http::header::HeaderValue,
    )> for HeaderMap
{
    fn from_iter<
        T: IntoIterator<
            Item = (
                &'a actix_web::http::header::HeaderName,
                &'a actix_web::http::header::HeaderValue,
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

#[cfg(test)]
mod test {
    use crate::HeaderMap;

    #[test]
    fn get_case_insensitive() {
        let mut header_map = HeaderMap::new();
        header_map.insert("key", "value");

        assert_eq!(header_map.get("key"), Some("value"));
        assert_eq!(header_map.get("KEY"), Some("value"));

        header_map.insert("KEY", "value_2");
        assert_eq!(header_map.get("key"), Some("value_2"));
        assert_eq!(header_map.get("KEY"), Some("value_2"));
    }
}
