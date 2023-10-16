use async_trait::async_trait;
use rand::Rng;
use std::collections::HashMap;
use thiserror::Error;

mod memory_session_store;
mod name_gen;
mod session;
mod session_allocator;

pub use memory_session_store::*;
pub use name_gen::{new_session_name, random_animal, random_six_char};
pub use session::*;
pub use session_allocator::*;

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

#[async_trait(?Send)]
pub trait StringStore {
    async fn get(&self, key: String) -> Result<Option<String>, SessionError>;
    async fn exists(&self, key: String) -> Result<bool, SessionError>;
    async fn put(&self, key: String, value: String) -> Result<(), SessionError>;
}

#[derive(PartialEq)]
pub enum NameKind {
    Animal,
    SixChar,
}

pub fn get_additional_headers(
    url: String,
    headers: &HashMap<String, String>,
    session_name: &str,
    destination_service_name: &str,
) -> HashMap<String, String> {
    let mut additional_headers = HashMap::new();

    if !headers.contains_key("traceparent") {
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
        additional_headers.insert("traceparent".to_string(), traceparent);
    }

    let tracestate = headers.get("tracestate");
    let linkup_session = format!("linkup-session={}", session_name,);
    match tracestate {
        Some(ts) if !ts.contains(&linkup_session) => {
            let new_tracestate = format!("{},{}", ts, linkup_session);
            additional_headers.insert("tracestate".to_string(), new_tracestate);
        }
        None => {
            let new_tracestate = linkup_session;
            additional_headers.insert("tracestate".to_string(), new_tracestate);
        }
        _ => {}
    }

    if !headers.contains_key("linkup-destination") {
        additional_headers.insert(
            "linkup-destination".to_string(),
            destination_service_name.to_string(),
        );
    }

    if !headers.contains_key("x-forwarded-host") {
        additional_headers.insert(
            "x-forwarded-host".to_string(),
            get_target_domain(&url, session_name),
        );
    }

    additional_headers
}

pub fn additional_response_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();

    headers.insert(
        "Access-Control-Allow-Methods".to_string(),
        "GET, POST, PUT, PATCH, DELETE, HEAD, CONNECT, TRACE, OPTIONS".to_string(),
    );
    headers.insert("Access-Control-Allow-Origin".to_string(), "*".to_string());
    headers.insert("Access-Control-Allow-Headers".to_string(), "*".to_string());
    headers.insert("Access-Control-Max-Age".to_string(), "86400".to_string());

    headers
}

// Returns a (name, url) pair for the destination service, if the request could be served by the config
pub fn get_target_service(
    url: String,
    headers: HashMap<String, String>,
    config: &Session,
    session_name: &str,
) -> Option<(String, String)> {
    let mut target = Url::parse(&url).unwrap();
    // Ensure always the default port, even when the local server is hit first
    target
        .set_port(None)
        .expect("setting port to None is always valid");
    let path = target.path();

    // If there was a destination created in a previous linkup, we don't want to
    // re-do path rewrites, so we use the destination service.
    if let Some(destination_service) = headers.get("linkup-destination") {
        if let Some(service) = config.services.get(destination_service) {
            let target = redirect(target.clone(), &service.origin, Some(path.to_string()));
            return Some((destination_service.to_string(), String::from(target)));
        }
    }

    let url_target = config.domains.get(&get_target_domain(&url, session_name));

    // Forwarded hosts persist over the tunnel
    let forwarded_host_target = config.domains.get(&get_target_domain(
        headers.get("x-forwarded-host").unwrap_or(
            headers
                .get("X-Forwarded-Host")
                .unwrap_or(&"does-not-exist".to_string()),
        ),
        session_name,
    ));

    // This is more for e2e tests to work
    let referer_target = config.domains.get(&get_target_domain(
        headers
            .get("referer")
            .unwrap_or(&"does-not-exist".to_string()),
        session_name,
    ));

    // This one is for redirects, where the referer doesn't exist
    let origin_target = config.domains.get(&get_target_domain(
        headers
            .get("origin")
            .unwrap_or(&"does-not-exist".to_string()),
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
            return Some((service_name, String::from(target)));
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
        String::from("")
    } else {
        String::from(parts[0])
    }
}

fn extract_tracestate_session(tracestate: &str) -> String {
    extrace_tracestate(tracestate, String::from("linkup-session"))
}

fn extrace_tracestate(tracestate: &str, linkup_key: String) -> String {
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
    use std::sync::Arc;

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
            }
        ]
    }
    "#;

    #[tokio::test]
    async fn test_get_request_session_by_subdomain() {
        let sessions = SessionAllocator::new(Arc::new(MemoryStringStore::new()));

        let config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let config: Session = config_value.try_into().unwrap();

        let name = sessions
            .store_session(config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        // Normal subdomain
        sessions
            .get_request_session(format!("{}.example.com", name), HashMap::new())
            .await
            .unwrap();

        // Referer
        let mut referer_headers: HashMap<String, String> = HashMap::new();
        // TODO check header capitalization
        referer_headers.insert(
            "referer".to_string(),
            format!("http://{}.example.com", name),
        );
        sessions
            .get_request_session("example.com".to_string(), referer_headers)
            .await
            .unwrap();

        // Origin
        let mut origin_headers: HashMap<String, String> = HashMap::new();
        origin_headers.insert("origin".to_string(), format!("http://{}.example.com", name));
        sessions
            .get_request_session("example.com".to_string(), origin_headers)
            .await
            .unwrap();

        // Trace state
        let mut trace_headers: HashMap<String, String> = HashMap::new();
        trace_headers.insert(
            "tracestate".to_string(),
            format!("some-other=xyz,linkup-session={}", name),
        );
        sessions
            .get_request_session("example.com".to_string(), trace_headers)
            .await
            .unwrap();

        let mut trace_headers_two: HashMap<String, String> = HashMap::new();
        trace_headers_two.insert("tracestate".to_string(), format!("linkup-session={}", name));
        sessions
            .get_request_session("example.com".to_string(), trace_headers_two)
            .await
            .unwrap();
    }

    #[test]
    fn test_get_additional_headers() {
        let session_name = String::from("tiny-cow");
        let destination_service_name = String::from("frontend");
        let headers = HashMap::new();
        let add_headers = get_additional_headers(
            "https://tiny-cow.example.com/abc-xyz".to_string(),
            &headers,
            &session_name,
            &destination_service_name,
        );

        assert_eq!(add_headers.get("traceparent").unwrap().len(), 55);
        assert_eq!(
            add_headers.get("tracestate").unwrap(),
            "linkup-session=tiny-cow"
        );
        assert_eq!(add_headers.get("x-forwarded-host").unwrap(), "example.com");
        assert_eq!(add_headers.get("linkup-destination").unwrap(), "frontend");

        let mut already_headers: HashMap<String, String> = HashMap::new();
        already_headers.insert("traceparent".to_string(), "anything".to_string());
        already_headers.insert(
            "tracestate".to_string(),
            "linkup-session=tiny-cow".to_string(),
        );
        already_headers.insert("X-Forwarded-Host".to_string(), "example.com".to_string());
        already_headers.insert("linkup-destination".to_string(), "frontend".to_string());
        let add_headers = get_additional_headers(
            "https://abc.some-tunnel.com/abc-xyz".to_string(),
            &already_headers,
            &session_name,
            &destination_service_name,
        );

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("tracestate").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert!(add_headers.get("linkup-destination").is_none());

        let mut already_headers_two: HashMap<String, String> = HashMap::new();
        already_headers_two.insert("traceparent".to_string(), "anything".to_string());
        already_headers_two.insert("tracestate".to_string(), "other-service=32".to_string());
        already_headers_two.insert("X-Forwarded-Host".to_string(), "example.com".to_string());
        let add_headers = get_additional_headers(
            "https://abc.some-tunnel.com/abc-xyz".to_string(),
            &already_headers_two,
            &session_name,
            &destination_service_name,
        );

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert_eq!(
            add_headers.get("tracestate").unwrap(),
            "other-service=32,linkup-session=tiny-cow"
        );
    }

    #[test]
    fn test_get_target_domain() {
        let url1 = "tiny-cow.example.com/path/to/page.html".to_string();
        let url2 = "api.example.com".to_string();
        let url3 = "https://tiny-cow.example.com/a/b/c?a=b".to_string();

        assert_eq!(get_target_domain(&url1, "tiny-cow"), "example.com");
        assert_eq!(get_target_domain(&url2, "tiny-cow"), "api.example.com");
        assert_eq!(get_target_domain(&url3, "tiny-cow"), "example.com");
    }

    #[tokio::test]
    async fn test_get_target_url() {
        let sessions = SessionAllocator::new(Arc::new(MemoryStringStore::new()));

        let input_config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let input_config: Session = input_config_value.try_into().unwrap();

        let name = sessions
            .store_session(input_config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        let (name, config) = sessions
            .get_request_session(format!("{}.example.com", name), HashMap::new())
            .await
            .unwrap();

        // Standard named subdomain
        assert_eq!(
            get_target_service(
                format!("http://{}.example.com/?a=b", &name),
                HashMap::new(),
                &config,
                &name
            )
            .unwrap(),
            (
                "frontend".to_string(),
                "http://localhost:8000/?a=b".to_string()
            ),
        );
        // With path
        assert_eq!(
            get_target_service(
                format!("http://{}.example.com/a/b/c/?a=b", &name),
                HashMap::new(),
                &config,
                &name
            )
            .unwrap(),
            (
                "frontend".to_string(),
                "http://localhost:8000/a/b/c/?a=b".to_string()
            ),
        );
        // Test rewrites
        assert_eq!(
            get_target_service(
                format!("http://{}.example.com/foo/b/c/?a=b", &name),
                HashMap::new(),
                &config,
                &name
            )
            .unwrap(),
            (
                "frontend".to_string(),
                "http://localhost:8000/bar/b/c/?a=b".to_string()
            ),
        );
        // Test domain routes
        assert_eq!(
            get_target_service(
                format!("http://{}.example.com/api/v1/?a=b", &name),
                HashMap::new(),
                &config,
                &name
            )
            .unwrap(),
            (
                "backend".to_string(),
                "http://localhost:8001/api/v1/?a=b".to_string()
            )
        );
        // Test no named subdomain
        assert_eq!(
            get_target_service(
                "http://api.example.com/api/v1/?a=b".to_string(),
                HashMap::new(),
                &config,
                &name
            )
            .unwrap(),
            (
                "backend".to_string(),
                "http://localhost:8001/api/v1/?a=b".to_string()
            )
        );
    }

    #[tokio::test]
    async fn test_repeatable_rewritten_routes() {
        let sessions = SessionAllocator::new(Arc::new(MemoryStringStore::new()));

        let input_config_value: serde_json::Value = serde_json::from_str(CONF_STR).unwrap();
        let input_config: Session = input_config_value.try_into().unwrap();

        let name = sessions
            .store_session(input_config, NameKind::Animal, "".to_string())
            .await
            .unwrap();

        let (name, config) = sessions
            .get_request_session(format!("{}.example.com", name), HashMap::new())
            .await
            .unwrap();

        // Case is, target service on the remote side is a tunnel.
        // If the path gets rewritten once remotely, it can throw off finding
        // the right service in the local server

        let (service, service_url) = get_target_service(
            "http://example.com/api/v2/user".to_string(),
            HashMap::new(),
            &config,
            &name,
        )
        .unwrap();

        // First request as normal
        assert_eq!(service, "backend");
        assert_eq!(service_url, "http://localhost:8001/user");

        let extra_headers = get_additional_headers(
            "http://example.com/api/v2/user".to_string(),
            &HashMap::new(),
            &name,
            &service,
        );

        let (service, service_url) = get_target_service(
            "http://localhost:8001/user".to_string(),
            extra_headers,
            &config,
            &name,
        )
        .unwrap();

        // Second request should have the same outcome
        // The secret sauce should be in the extra headers that have been propogated
        assert_eq!(service, "backend");
        assert_eq!(service_url, "http://localhost:8001/user");
    }
}
