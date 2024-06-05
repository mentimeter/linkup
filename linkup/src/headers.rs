use http::{HeaderMap as HttpHeaderMap, HeaderValue as HttpHeaderValue};
use std::collections::HashMap;

use unicase::UniCase;

#[derive(Debug)]
pub struct HeaderMap(HashMap<UniCase<String>, String>);

pub enum HeaderName {
    ForwardedHost,
    TraceParent,
    TraceState,
    Baggage,
    LinkupDestination,
    Referer,
    Origin,
    Host,
    SetCookie,
}

impl From<HeaderName> for UniCase<String> {
    fn from(value: HeaderName) -> Self {
        match value {
            HeaderName::ForwardedHost => "x-forwarded-host".into(),
            HeaderName::TraceParent => "traceparent".into(),
            HeaderName::TraceState => "tracestate".into(),
            HeaderName::Baggage => "baggage".into(),
            HeaderName::LinkupDestination => "linkup-destination".into(),
            HeaderName::Referer => "referer".into(),
            HeaderName::Origin => "origin".into(),
            HeaderName::Host => "host".into(),
            HeaderName::SetCookie => "set-cookie".into(),
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
        let unicase_key = key.into();
        if unicase_key == HeaderName::SetCookie.into() && self.0.contains_key(&unicase_key) {
            let cookies = self.0.get(&unicase_key).unwrap();
            let append_cookie = format!("{}, {}", cookies, value.to_string());
            return self.0.insert(unicase_key, append_cookie);
        }

        self.0.insert(unicase_key, value.to_string())
    }

    pub fn extend(&mut self, iter: &HeaderMap) {
        self.0.extend(iter)
    }

    pub fn remove(&mut self, key: impl Into<UniCase<String>>) -> Option<String> {
        self.0.remove(&key.into())
    }

    fn from_http_headers(http_headers: &HttpHeaderMap) -> Self {
        let mut linkup_headers = HeaderMap::new();
        for (key, value) in http_headers.iter() {
            if let Ok(value_str) = value.to_str() {
                linkup_headers.insert(key.to_string(), value_str);
            }
        }
        linkup_headers
    }
}

impl From<&HttpHeaderMap> for HeaderMap {
    fn from(http_headers: &HttpHeaderMap) -> Self {
        HeaderMap::from_http_headers(http_headers)
    }
}

impl From<HttpHeaderMap> for HeaderMap {
    fn from(http_headers: HttpHeaderMap) -> Self {
        HeaderMap::from_http_headers(&http_headers)
    }
}

impl From<HeaderMap> for HttpHeaderMap {
    fn from(linkup_headers: HeaderMap) -> Self {
        let mut http_headers = HttpHeaderMap::new();
        for (key, value) in linkup_headers.into_iter() {
            if let Ok(http_value) = HttpHeaderValue::from_str(&value) {
                if let Ok(http_key) = http::header::HeaderName::from_bytes(key.as_bytes()) {
                    http_headers.insert(http_key, http_value);
                }
            }
        }
        http_headers
    }
}
