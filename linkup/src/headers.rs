use http::{HeaderMap as HttpHeaderMap, HeaderValue as HttpHeaderValue};
use std::collections::HashMap;

use unicase::UniCase;

#[derive(Debug)]
pub struct HeaderMap(HashMap<UniCase<String>, String>);

pub enum HeaderName {
    ForwardedHost,
    TraceParent,
    TraceState,
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

pub fn normalize_cookie_header(http_headers: &mut HttpHeaderMap) {
    let raw_values: Vec<&str> = http_headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();

    if raw_values.is_empty() {
        return;
    }

    // If there's only one Cookie header field and it doesn't contain a comma,
    // there's nothing to normalize.
    if raw_values.len() == 1 && !raw_values[0].contains(',') {
        return;
    }

    // RFC 7540 §8.1.2.5: multiple Cookie header fields MUST be concatenated
    // using the delimiter "; " when passed into a non-HTTP/2 context.
    //
    // Some stacks incorrectly join Cookie fields with commas; commas are not
    // valid Cookie delimiters (RFC 6265), so treat them as split points.
    let mut cookie_parts: Vec<&str> = Vec::new();
    for raw in raw_values {
        for part in raw.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                cookie_parts.push(part);
            }
        }
    }

    if cookie_parts.len() <= 1 {
        return;
    }

    let combined = cookie_parts.join("; ");
    http_headers.remove(http::header::COOKIE);
    if let Ok(value) = HttpHeaderValue::from_str(&combined) {
        http_headers.insert(http::header::COOKIE, value);
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

#[cfg(test)]
mod tests {
    use super::normalize_cookie_header;
    use http::{header::COOKIE, HeaderMap, HeaderValue};

    #[test]
    fn normalizes_multiple_cookie_headers_with_semicolon() {
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("a=b"));
        headers.append(COOKIE, HeaderValue::from_static("c=d"));

        normalize_cookie_header(&mut headers);

        let cookies: Vec<_> = headers.get_all(COOKIE).iter().collect();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].to_str().unwrap(), "a=b; c=d");
    }

    #[test]
    fn leaves_single_cookie_header_unchanged() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("a=b; c=d"));

        normalize_cookie_header(&mut headers);

        let cookies: Vec<_> = headers.get_all(COOKIE).iter().collect();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].to_str().unwrap(), "a=b; c=d");
    }

    #[test]
    fn fixes_comma_joined_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("a=b, c=d"));

        normalize_cookie_header(&mut headers);

        let cookies: Vec<_> = headers.get_all(COOKIE).iter().collect();
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].to_str().unwrap(), "a=b; c=d");
    }
}
