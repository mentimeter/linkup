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
                if UniCase::new(&header_name) == HeaderName::SetCookie.into() {
                    let cookies = value.split(", ").collect::<Vec<&str>>();
                    for cookie in cookies {
                        if let Ok(header_value) = reqwest::header::HeaderValue::from_str(cookie) {
                            header_map.append(header_name.clone(), header_value);
                        }
                    }
                    continue;
                }

                if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&value) {
                    header_map.insert(header_name, header_value);
                }
            }
        }

        header_map
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

#[cfg(feature = "worker")]
pub fn unpack_cookie_header(header: String) -> Vec<String> {
    if header.is_empty() {
        return Vec::new();
    }

    let parts: Vec<&str> = header.split(',').collect();
    let mut cookies = Vec::new();
    let mut i = 0;

    while i < parts.len() {
        // Check if the current part ends with the start of an Expires attribute
        if parts[i].trim().ends_with("Expires=Mon")
            || parts[i].trim().ends_with("Expires=Tue")
            || parts[i].trim().ends_with("Expires=Wed")
            || parts[i].trim().ends_with("Expires=Thu")
            || parts[i].trim().ends_with("Expires=Fri")
            || parts[i].trim().ends_with("Expires=Sat")
            || parts[i].trim().ends_with("Expires=Sun")
        {
            // If it does, and there's a next part, concatenate the current and next parts
            if i + 1 < parts.len() {
                cookies.push(format!("{}, {}", parts[i].trim(), parts[i + 1].trim()));
                i += 2; // Skip the next part since it's been concatenated
                continue;
            }
        }

        // If not handling an Expires attribute, or it's the last part, add the current part as a cookie
        cookies.push(parts[i].trim().to_string());
        i += 1;
    }

    cookies
}

#[cfg(test)]
#[cfg(feature = "reqwest")]
mod test {
    use super::*;
    use reqwest::header::{HeaderMap as ReqwestHeaderMap, HeaderValue, SET_COOKIE};

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

    #[test]
    fn add_folded_cookie_headers() {
        let mut header_map = HeaderMap::new();

        // Cloudflare Workers-rs folds set-cookie headers into a single header
        header_map.insert("set-cookie".to_string(), "cookie1=value1, cookie2=value2");

        let reqwest_header_map: ReqwestHeaderMap = header_map.into();
        let cookies: Vec<&HeaderValue> = reqwest_header_map.get_all(SET_COOKIE).iter().collect();

        assert_eq!(reqwest_header_map.len(), 2);
        assert!(cookies.contains(&&HeaderValue::from_static("cookie1=value1")));
        assert!(cookies.contains(&&HeaderValue::from_static("cookie2=value2")));
    }

    #[test]
    fn add_multi_cookie_headers() {
        let mut header_map = HeaderMap::new();

        header_map.insert("set-cookie".to_string(), "cookie1=value1");
        header_map.insert("set-cookie".to_string(), "cookie2=value2");

        let reqwest_header_map: ReqwestHeaderMap = header_map.into();

        assert_eq!(reqwest_header_map.len(), 2);
        let cookies: Vec<&HeaderValue> = reqwest_header_map.get_all(SET_COOKIE).iter().collect();
        assert!(cookies.contains(&&HeaderValue::from_static("cookie1=value1")));
        assert!(cookies.contains(&&HeaderValue::from_static("cookie2=value2")));
    }
}

#[cfg(test)]
#[cfg(feature = "worker")]
mod tests {
    use super::*;

    #[test]
    fn handle_response_without_set_cookie_headers() {
        let header = String::new(); // Simulates a response without Set-Cookie headers
        let cookies = unpack_cookie_header(header);
        assert!(cookies.is_empty());
    }

    #[test]
    fn handle_response_with_single_set_cookie_header_without_expires() {
        let header = "sessionId=abc123; Path=/; HttpOnly".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(cookies, vec!["sessionId=abc123; Path=/; HttpOnly"]);
    }

    #[test]
    fn handle_multiple_set_cookie_headers_without_merging_them_unnecessarily() {
        let header = "sessionId=abc123; Path=/; HttpOnly, theme=dark; Path=/".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(
            cookies,
            vec!["sessionId=abc123; Path=/; HttpOnly", "theme=dark; Path=/"]
        );
    }

    #[test]
    fn correctly_merge_set_cookie_headers_when_expires_attribute_is_present() {
        let header = "sessionId=abc123; Path=/; Expires=Fri, 31 Dec 9999 23:59:59 GMT, theme=dark; Path=/; Expires=Fri, 31 Dec 9999 23:59:59 GMT".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(
            cookies,
            vec![
                "sessionId=abc123; Path=/; Expires=Fri, 31 Dec 9999 23:59:59 GMT",
                "theme=dark; Path=/; Expires=Fri, 31 Dec 9999 23:59:59 GMT"
            ]
        );
    }

    #[test]
    fn handle_when_cookies_have_empty_values() {
        let header = "sessionId=; Path=/; Expires=Mon, 1 Jan 1970 00:00:00 GMT, theme=; Path=/; Expires=Mon, 1 Jan 1970 00:00:00 GMT".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(
            cookies,
            vec![
                "sessionId=; Path=/; Expires=Mon, 1 Jan 1970 00:00:00 GMT",
                "theme=; Path=/; Expires=Mon, 1 Jan 1970 00:00:00 GMT"
            ]
        );
    }

    #[test]
    fn handle_cookies_with_quoted_values() {
        let header = "name=\"an example value\"; Path=/; HttpOnly".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(cookies, vec!["name=\"an example value\"; Path=/; HttpOnly"]);
    }

    #[test]
    fn handle_cookies_with_multiple_attributes() {
        let header = "id=123; Path=/; Secure; HttpOnly; SameSite=Strict".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(
            cookies,
            vec!["id=123; Path=/; Secure; HttpOnly; SameSite=Strict"]
        );
    }

    #[test]
    fn handle_cookies_with_the_max_age_attribute() {
        let header = "id=123; Path=/; Max-Age=3600; HttpOnly".to_string();
        let cookies = unpack_cookie_header(header);
        assert_eq!(cookies, vec!["id=123; Path=/; Max-Age=3600; HttpOnly"]);
    }
}
