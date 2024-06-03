mod headers;
mod memory_session_store;
mod name_gen;
mod session;
mod session_allocator;

use std::future::Future;

use http::{HeaderMap as HttpHeaderMap, HeaderValue as HttpHeaderValue};
use rand::Rng;
use thiserror::Error;

pub use headers::{HeaderMap, HeaderName};
pub use memory_session_store::*;
pub use name_gen::{random_animal, random_six_char};
pub use session::*;
pub use session_allocator::*;

#[cfg(feature = "worker")]
pub use headers::unpack_cookie_header;

use url::Url;

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("no session found for request {0}")]
    NoSuchSession(String),
    #[error("Could not get config: {0}")]
    GetError(String),
    #[error("Could not put config: {0}")]
    PutError(String),
    #[error("Invalid stored config: {0}")]
    ConfigErr(String),
}

// Since this trait is theoretically public (even though, the idea is for it to be used by the other modules within
// this workspace), we should return `impl Future` instead of having `async fn` so that we can add and ensure
// any desired bounds.
pub trait StringStore {
    fn get(&self, key: String) -> impl Future<Output = Result<Option<String>, SessionError>>;
    fn exists(&self, key: String) -> impl Future<Output = Result<bool, SessionError>>;
    fn put(&self, key: String, value: String) -> impl Future<Output = Result<(), SessionError>>;
}

#[derive(PartialEq)]
pub enum NameKind {
    Animal,
    SixChar,
}

pub fn get_additional_headers(
    url: &str,
    headers: &HeaderMap,
    session_name: &str,
    target_service: &TargetService,
) -> HeaderMap {
    let mut additional_headers = HeaderMap::new();

    if !headers.contains_key(HeaderName::TraceParent) {
        let mut rng = rand::thread_rng();
        let trace: [u8; 16] = rng.gen();
        let parent: [u8; 8] = rng.gen();
        let version: [u8; 1] = [0];
        let flags: [u8; 1] = [0];

        let trace_hex = hex::encode(trace);
        let parent_hex = hex::encode(parent);
        let version_hex = hex::encode(version);
        let flags_hex = hex::encode(flags);

        let traceparent = format!("{}-{}-{}-{}", version_hex, trace_hex, parent_hex, flags_hex);
        additional_headers.insert(HeaderName::TraceParent, traceparent);
    }

    let tracestate = headers.get(HeaderName::TraceState);
    let linkup_session = format!("linkup-session={}", session_name,);
    match tracestate {
        Some(ts) if !ts.contains(&linkup_session) => {
            let new_tracestate = format!("{},{}", ts, linkup_session);
            additional_headers.insert(HeaderName::TraceState, new_tracestate);
        }
        None => {
            let new_tracestate = linkup_session;
            additional_headers.insert(HeaderName::TraceState, new_tracestate);
        }
        _ => {}
    }

    let baggage = headers.get(HeaderName::Baggage);
    let linkup_session = format!("linkup-session={}", session_name,);
    match baggage {
        Some(ts) if !ts.contains(&linkup_session) => {
            let new_baggage = format!("{},{}", ts, linkup_session);
            additional_headers.insert(HeaderName::Baggage, new_baggage);
        }
        None => {
            let new_baggage = linkup_session;
            additional_headers.insert(HeaderName::Baggage, new_baggage);
        }
        _ => {}
    }

    if !headers.contains_key(HeaderName::LinkupDestination) {
        additional_headers.insert(HeaderName::LinkupDestination, &target_service.name);
    }

    if !headers.contains_key(HeaderName::ForwardedHost) {
        additional_headers.insert(
            HeaderName::ForwardedHost,
            format!("{}.{}", session_name, get_target_domain(url, session_name)),
        );
    }

    additional_headers
}

pub fn additional_response_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(
        "Access-Control-Allow-Methods",
        "GET, POST, PUT, PATCH, DELETE, HEAD, CONNECT, TRACE, OPTIONS",
    );
    headers.insert("Access-Control-Allow-Origin", "*");
    headers.insert("Access-Control-Allow-Headers", "*");
    headers.insert("Access-Control-Max-Age", "86400");

    headers
}

pub fn allow_all_cors() -> HttpHeaderMap {
    let mut headers = HttpHeaderMap::new();

    headers.insert(
        "Access-Control-Allow-Methods",
        HttpHeaderValue::from_static(
            "GET, POST, PUT, PATCH, DELETE, HEAD, CONNECT, TRACE, OPTIONS",
        ),
    );
    headers.insert(
        "Access-Control-Allow-Origin",
        HttpHeaderValue::from_static("*"),
    );
    headers.insert(
        "Access-Control-Allow-Headers",
        HttpHeaderValue::from_static("*"),
    );
    headers.insert(
        "Access-Control-Max-Age",
        HttpHeaderValue::from_static("86400"),
    );

    headers
}

#[derive(Debug, PartialEq)]
pub struct TargetService {
    pub name: String,
    pub url: String,
}

// TODO(ostenbom): Accept a http::Uri instead of a string. Change TargetService to use Uri instead of String.
// Returns a (name, url) pair for the destination service, if the request could be served by the config
pub fn get_target_service(
    url: &str,
    headers: &HeaderMap,
    config: &Session,
    session_name: &str,
) -> Option<TargetService> {
    let mut target = Url::parse(url).unwrap();
    // Ensure always the default port, even when the local server is hit first
    target
        .set_port(None)
        .expect("setting port to None is always valid");
    let path = target.path();

    // If there was a destination created in a previous linkup, we don't want to
    // re-do path rewrites, so we use the destination service.
    if let Some(destination_service) = headers.get(HeaderName::LinkupDestination) {
        if let Some(service) = config.services.get(destination_service) {
            let target = redirect(target.clone(), &service.origin, Some(path.to_string()));
            return Some(TargetService {
                name: destination_service.to_string(),
                url: target.to_string(),
            });
        }
    }

    let url_target = config.domains.get(&get_target_domain(url, session_name));

    // Forwarded hosts persist over the tunnel
    let forwarded_host_target = config.domains.get(&get_target_domain(
        headers.get_or_default(HeaderName::ForwardedHost, "does-not-exist"),
        session_name,
    ));

    // This is more for e2e tests to work
    let referer_target = config.domains.get(&get_target_domain(
        headers.get_or_default(HeaderName::Referer, "does-not-exist"),
        session_name,
    ));

    // This one is for redirects, where the referer doesn't exist
    let origin_target = config.domains.get(&get_target_domain(
        headers.get_or_default(HeaderName::Origin, "does-not-exist"),
        session_name,
    ));

    let target_domain = if url_target.is_some() {
        url_target
    } else if forwarded_host_target.is_some() {
        forwarded_host_target
    } else if referer_target.is_some() {
        referer_target
    } else {
        origin_target
    };

    if let Some(domain) = target_domain {
        let service_name = domain
            .routes
            .iter()
            .find_map(|route| {
                if route.path.is_match(path) {
                    Some(route.service.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| domain.default_service.clone());

        if let Some(service) = config.services.get(&service_name) {
            let mut new_path = path.to_string();
            for modifier in &service.rewrites {
                if modifier.source.is_match(&new_path) {
                    new_path = modifier
                        .source
                        .replace_all(&new_path, &modifier.target)
                        .to_string();
                }
            }

            let target = redirect(target, &service.origin, Some(new_path));
            return Some(TargetService {
                name: service_name,
                url: target.to_string(),
            });
        }
    }

    None
}

fn redirect(mut target: Url, source: &Url, path: Option<String>) -> Url {
    target.set_host(source.host_str()).unwrap();
    target.set_scheme(source.scheme()).unwrap();

    if let Some(port) = source.port() {
        target.set_port(Some(port)).unwrap();
    }

    if let Some(p) = path {
        target.set_path(&p);
    }

    target
}

fn get_target_domain(url: &str, session_name: &str) -> String {
    let without_schema = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    let domain_with_path: String = if first_subdomain(url) == *session_name {
        without_schema
            .strip_prefix(&format!("{}.", session_name))
            .map(String::from)
            .unwrap_or_else(|| without_schema.to_string())
    } else {
        without_schema.to_string()
    };

    domain_with_path.split('/').collect::<Vec<_>>()[0].to_string()
}

fn first_subdomain(url: &str) -> String {
    let without_schema = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let parts: Vec<&str> = without_schema.split('.').collect();
    if parts.len() <= 2 {
        String::from("DOES-NOT-EXIST")
    } else {
        String::from(parts[0])
    }
}

fn extract_tracestate_session(tracestate: &str) -> String {
    extract_tracestate(tracestate, String::from("linkup-session"))
}

fn extract_tracestate(tracestate: &str, linkup_key: String) -> String {
    tracestate
        .split(',')
        .filter_map(|kv| {
            let (key, value) = kv.split_once('=')?;
            if key.trim() == linkup_key {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF_STR: &str = r#"
    {
        "session_token": "abcxyz",
        "services": [
            {
                "name": "frontend",
                "location": "http://localhost:8000",
                "rewrites": [
                    {
                        "source": "/foo/(.*)",
                        "target": "/bar/$1"
                    }
                ]
            },
            {
                "name": "other-frontend",
                "location": "http://localhost:5000"
            },
            {
                "name": "backend",
                "rewrites": [
                    {
                        "source": "/api/v2/(.*)",
                        "target": "/$1"
                    }
                ],
                "location": "http://localhost:8001/"
            }
        ],
        "domains": [
            {
                "domain": "example.com",
                "default_service": "frontend",
                "routes": [
                    {
                        "path": "/api/v1/.*",
                        "service": "backend"
                    },
                    {
                        "path": "/api/v2/.*",
                        "service": "backend"
                    }
                ]
            },
            {
                "domain": "api.example.com",
                "default_service": "backend"
            },
            {
                "domain": "other-example.com",
                "default_service": "other-frontend"
            }
        ]
    }
    "#;

    #[tokio::test]
    async fn test_get_request_session_by_subdomain() {
        let string_store = MemoryStringStore::default();
        let sessions = SessionAllocator::new(&string_store);

        let config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let config: Session = config_value.try_into().unwrap();

        let name = sessions
            .store_session(config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        // Normal subdomain
        sessions
            .get_request_session(&format!("{}.example.com", name), &HeaderMap::new())
            .await
            .unwrap();

        // Referer
        let mut referer_headers = HeaderMap::new();
        referer_headers.insert("referer", format!("http://{}.example.com", name));
        sessions
            .get_request_session("example.com", &referer_headers)
            .await
            .unwrap();

        // Origin
        let mut origin_headers = HeaderMap::new();
        origin_headers.insert("origin", format!("http://{}.example.com", name));
        sessions
            .get_request_session("example.com", &origin_headers)
            .await
            .unwrap();

        // Trace state
        let mut trace_headers = HeaderMap::new();
        trace_headers.insert(
            HeaderName::TraceState,
            format!("some-other=xyz,linkup-session={}", name),
        );
        sessions
            .get_request_session("example.com", &trace_headers)
            .await
            .unwrap();

        let mut trace_headers_two = HeaderMap::new();
        trace_headers_two.insert(HeaderName::TraceState, format!("linkup-session={}", name));
        sessions
            .get_request_session("example.com", &trace_headers_two)
            .await
            .unwrap();

        // Test with a single baggage header
        let mut baggage_headers = HeaderMap::new();
        baggage_headers.insert(
            HeaderName::Baggage,
            format!("other-context=12345,linkup-session={}", name),
        );
        sessions
            .get_request_session("example.com", &baggage_headers)
            .await
            .unwrap();

        let mut baggage_headers_two = HeaderMap::new();
        baggage_headers_two.insert(HeaderName::Baggage, format!("linkup-session={}", name));
        sessions
            .get_request_session("example.com", &baggage_headers_two)
            .await
            .unwrap();
    }

    #[test]
    fn test_get_additional_headers() {
        let session_name = String::from("tiny-cow");
        let target_service = TargetService {
            name: String::from("frontend"),
            url: String::from("http://example.com"),
        };
        let headers = HeaderMap::new();
        let add_headers = get_additional_headers(
            "https://tiny-cow.example.com/abc-xyz",
            &headers,
            &session_name,
            &target_service,
        );

        assert_eq!(add_headers.get(HeaderName::TraceParent).unwrap().len(), 55);
        assert_eq!(
            add_headers.get(HeaderName::TraceState).unwrap(),
            "linkup-session=tiny-cow"
        );
        assert_eq!(
            add_headers.get(HeaderName::Baggage).unwrap(),
            "linkup-session=tiny-cow"
        );
        assert_eq!(
            add_headers.get(HeaderName::ForwardedHost).unwrap(),
            "tiny-cow.example.com"
        );
        assert_eq!(
            add_headers.get(HeaderName::LinkupDestination).unwrap(),
            "frontend"
        );

        let mut already_headers = HeaderMap::new();
        already_headers.insert(HeaderName::TraceParent, "anything");
        already_headers.insert(HeaderName::TraceState, "linkup-session=tiny-cow");
        already_headers.insert(HeaderName::Baggage, "linkup-session=tiny-cow");
        already_headers.insert(HeaderName::ForwardedHost, "tiny-cow.example.com");
        already_headers.insert(HeaderName::LinkupDestination, "frontend");
        let add_headers = get_additional_headers(
            "https://abc.some-tunnel.com/abc-xyz",
            &already_headers,
            &session_name,
            &target_service,
        );

        assert!(add_headers.get(HeaderName::TraceParent).is_none());
        assert!(add_headers.get(HeaderName::TraceState).is_none());
        assert!(add_headers.get(HeaderName::Baggage).is_none());
        assert!(add_headers.get(HeaderName::ForwardedHost).is_none());
        assert!(add_headers.get(HeaderName::LinkupDestination).is_none());

        let mut already_headers_two = HeaderMap::new();
        already_headers_two.insert(HeaderName::TraceParent, "anything");
        already_headers_two.insert(HeaderName::TraceState, "other-service=32");
        already_headers_two.insert(HeaderName::Baggage, "other-service=32");
        already_headers_two.insert(HeaderName::ForwardedHost, "tiny-cow.example.com");
        let add_headers = get_additional_headers(
            "https://abc.some-tunnel.com/abc-xyz",
            &already_headers_two,
            &session_name,
            &target_service,
        );

        assert!(add_headers.get(HeaderName::TraceParent).is_none());
        assert!(add_headers.get(HeaderName::ForwardedHost).is_none());
        assert_eq!(
            add_headers.get(HeaderName::TraceState).unwrap(),
            "other-service=32,linkup-session=tiny-cow"
        );
        assert_eq!(
            add_headers.get(HeaderName::Baggage).unwrap(),
            "other-service=32,linkup-session=tiny-cow"
        );

        let mut already_headers_three = HeaderMap::new();
        already_headers_three.insert(HeaderName::ForwardedHost, "tiny-cow.example.com");
        let add_headers = get_additional_headers(
            "https://abc.some-tunnel.com/abc-xyz",
            &already_headers_three,
            &session_name,
            &target_service,
        );

        assert_eq!(add_headers.get(HeaderName::TraceParent).unwrap().len(), 55);
        assert_eq!(
            add_headers.get(HeaderName::TraceState).unwrap(),
            "linkup-session=tiny-cow"
        );
        assert_eq!(
            add_headers.get(HeaderName::Baggage).unwrap(),
            "linkup-session=tiny-cow"
        );
        assert!(add_headers.get(HeaderName::ForwardedHost).is_none());
    }

    #[test]
    fn test_get_target_domain() {
        let url1 = "tiny-cow.example.com/path/to/page.html";
        let url2 = "api.example.com";
        let url3 = "https://tiny-cow.example.com/a/b/c?a=b";

        assert_eq!(get_target_domain(url1, "tiny-cow"), "example.com");
        assert_eq!(get_target_domain(url2, "tiny-cow"), "api.example.com");
        assert_eq!(get_target_domain(url3, "tiny-cow"), "example.com");
    }

    #[tokio::test]
    async fn test_get_target_url() {
        let string_store = MemoryStringStore::default();
        let sessions = SessionAllocator::new(&string_store);

        let input_config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let input_config: Session = input_config_value.try_into().unwrap();

        let name = sessions
            .store_session(input_config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        let (name, config) = sessions
            .get_request_session(&format!("{}.example.com", name), &HeaderMap::new())
            .await
            .unwrap();

        // Standard named subdomain
        assert_eq!(
            get_target_service(
                &format!("http://{}.example.com/?a=b", &name),
                &HeaderMap::new(),
                &config,
                &name
            )
            .unwrap(),
            TargetService {
                name: String::from("frontend"),
                url: String::from("http://localhost:8000/?a=b")
            },
        );
        // With path
        assert_eq!(
            get_target_service(
                &format!("http://{}.example.com/a/b/c/?a=b", &name),
                &HeaderMap::new(),
                &config,
                &name
            )
            .unwrap(),
            TargetService {
                name: String::from("frontend"),
                url: String::from("http://localhost:8000/a/b/c/?a=b")
            },
        );
        // Test rewrites
        assert_eq!(
            get_target_service(
                &format!("http://{}.example.com/foo/b/c/?a=b", &name),
                &HeaderMap::new(),
                &config,
                &name
            )
            .unwrap(),
            TargetService {
                name: String::from("frontend"),
                url: String::from("http://localhost:8000/bar/b/c/?a=b")
            },
        );
        // Test domain routes
        assert_eq!(
            get_target_service(
                &format!("http://{}.example.com/api/v1/?a=b", &name),
                &HeaderMap::new(),
                &config,
                &name
            )
            .unwrap(),
            TargetService {
                name: String::from("backend"),
                url: String::from("http://localhost:8001/api/v1/?a=b")
            },
        );
        // Test no named subdomain
        assert_eq!(
            get_target_service(
                "http://api.example.com/api/v1/?a=b",
                &HeaderMap::new(),
                &config,
                &name
            )
            .unwrap(),
            TargetService {
                name: String::from("backend"),
                url: String::from("http://localhost:8001/api/v1/?a=b")
            },
        );
    }

    #[tokio::test]
    async fn test_repeatable_rewritten_routes() {
        let string_store = MemoryStringStore::default();
        let sessions = SessionAllocator::new(&string_store);

        let input_config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let input_config: Session = input_config_value.try_into().unwrap();

        let name = sessions
            .store_session(input_config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        let (name, config) = sessions
            .get_request_session(&format!("{}.example.com", name), &HeaderMap::new())
            .await
            .unwrap();

        // Case is, target service on the remote side is a tunnel.
        // If the path gets rewritten once remotely, it can throw off finding
        // the right service in the local server

        let target = get_target_service(
            "http://example.com/api/v2/user",
            &HeaderMap::new(),
            &config,
            &name,
        )
        .unwrap();

        // First request as normal
        assert_eq!(target.name, "backend");
        assert_eq!(target.url, "http://localhost:8001/user");

        let extra_headers = get_additional_headers(
            "http://example.com/api/v2/user",
            &HeaderMap::new(),
            &name,
            &target,
        );

        let target =
            get_target_service("http://localhost:8001/user", &extra_headers, &config, &name)
                .unwrap();

        // Second request should have the same outcome
        // The secret sauce should be in the extra headers that have been propogated
        assert_eq!(target.name, "backend");
        assert_eq!(target.url, "http://localhost:8001/user");
    }

    #[tokio::test]
    async fn test_iframable() {
        let string_store = MemoryStringStore::default();
        let sessions = SessionAllocator::new(&string_store);

        let input_config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let input_config: Session = input_config_value.try_into().unwrap();

        let name = sessions
            .store_session(input_config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::Referer,
            format!("https://{}.example.com/", name),
        );

        let (session_name, config) = sessions
            .get_request_session("http://other-example.com/", &headers)
            .await
            .unwrap();

        assert_eq!(session_name, name);

        let target =
            get_target_service("http://other-example.com/", &headers, &config, &name).unwrap();

        assert_eq!(target.name, "other-frontend");
        assert_eq!(target.url, "http://localhost:5000/");
    }
}
