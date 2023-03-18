use std::collections::HashMap;
use std::cell::RefCell;
use rand::Rng;
use thiserror::Error;

mod server_config;
mod name_gen;

use name_gen::{new_session_name};
pub use server_config::*;
use url::Url;

pub trait SessionStore {
    fn get(&self, name: &String) -> Option<ServerConfig>;
    fn new(&self, config: ServerConfig, name_kind: NameKind, desired_name: Option<String>) -> String;
}

pub struct MemorySessionStore {
    store: RefCell<HashMap<String, ServerConfig>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        MemorySessionStore {
            store: RefCell::new(HashMap::new()),
        }
    }
}

impl SessionStore for MemorySessionStore {
    fn get(&self, name: &String) -> Option<ServerConfig> {
        self.store.borrow().get(name).cloned()
    }

    fn new(&self, config: ServerConfig, name_kind: NameKind, desired_name: Option<String>) -> String {
        let exists_fn = |name: String| self.store.borrow().contains_key(&name);
        let key = new_session_name(name_kind, desired_name, &exists_fn);
        self.store.borrow_mut().insert(key.clone(), config);
        key
    }
}

#[derive(PartialEq)]
pub enum NameKind {
    Animal,
    SixChar,
}


#[derive(Error, Debug)]
pub enum SessionError {
    #[error("no session found for request {0}")]
    NoSuchSession(String) // Add known headers to error
}

pub fn get_request_session<T: SessionStore>(url: String, headers: HashMap<String, String>, store: &T) -> Result<(String, ServerConfig), SessionError> {
    let url_name = first_subdomain(&url);
    if let Some(config) = store.get(&url_name) {
        return Ok((url_name, config));
    }

    if let Some(referer) = headers.get("referer") {
        let referer_name = first_subdomain(referer);
        if let Some(config) = store.get(&referer_name) {
            return Ok((referer_name, config));
        }
    }

    if let Some(tracestate) = headers.get("tracestate") {
        let trace_name = extract_tracestate(tracestate);
        if let Some(config) = store.get(&trace_name) {
            return Ok((trace_name, config));
        }
    }

    Err(SessionError::NoSuchSession(url))
}

pub fn get_additional_headers(url: String, headers: HashMap<String, String>, name: &String) -> HashMap<String, String> {
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
    let serpress_session = format!("serpress-session={}", name);
    match tracestate {
        Some(ts) if !ts.contains(&serpress_session) => {
            let new_tracestate = format!("{},{}", ts, serpress_session);
            additional_headers.insert("tracestate".to_string(), new_tracestate);
        }
        None => {
            additional_headers.insert("tracestate".to_string(), serpress_session);
        }
        _ => {}
    }

    if !headers.contains_key("X-Forwarded-Host") {
        if let Ok(parsed_url) = Url::parse(&url) {
            if let Some(d) = parsed_url.domain() {
                let domain = d.to_string();
                if first_subdomain(&domain) == *name {
                    let forward_host = domain.strip_prefix(&format!("{}.", name)).map(String::from).unwrap_or_else(|| domain);
                    additional_headers.insert("X-Forwarded-Host".to_string(), forward_host);
                } else {
                    additional_headers.insert("X-Forwarded-Host".to_string(), domain);
                }
            }
        }
    }

    additional_headers
}

fn first_subdomain(url: &String) -> String {
    let without_schema = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
    let parts: Vec<&str> = without_schema.split('.').collect();
    if parts.len() <= 2 {
        String::from("")
    } else {
        String::from(parts[0])
    }
}

fn extract_tracestate(tracestate: &String) -> String {
    tracestate
        .split(',')
        .filter_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            let key = parts.next()?;
            let value = parts.next()?;
            if key.trim() == "serpress-session" {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .next()
        .unwrap_or_else(|| "".to_string())
}


#[cfg(test)]
mod tests {
    use super::*;


    const CONF_STR: &str = r#"
    services:
      - name: frontend
        location: http://localhost:8000
        path_modifiers:
          - source: /foo/(.*)
            target: /bar/$1
      - name: backend
        location: http://localhost:8001/
    domains:
      - domain: example.com
        default_service: frontend
        routes:
          - path: /api/v1/.*
            service: backend
      - domain: api.example.com
        default_service: backend
    "#;

    #[test]
    fn test_get_request_session_by_subdomain() {
        let session_store = MemorySessionStore::new();

        let config = new_server_config(String::from(CONF_STR)).unwrap();

        let name = session_store.new(config, NameKind::Animal, None);

        // Normal subdomain
        get_request_session(format!("{}.example.com", name), HashMap::new(), &session_store).unwrap();

        // Referer
        let mut referer_headers: HashMap<String, String> = HashMap::new();
        // TODO check header capitalization
        referer_headers.insert(format!("referer"), format!("http://{}.example.com", name));
        get_request_session(format!("example.com"), referer_headers, &session_store).unwrap();

        // Trace state
        let mut trace_headers: HashMap<String, String> = HashMap::new();
        trace_headers.insert(format!("tracestate"), format!("some-other=xyz,serpress-session={}", name));
        get_request_session(format!("example.com"), trace_headers, &session_store).unwrap();

        let mut trace_headers_two: HashMap<String, String> = HashMap::new();
        trace_headers_two.insert(format!("tracestate"), format!("serpress-session={}", name));
        get_request_session(format!("example.com"), trace_headers_two, &session_store).unwrap();
    }

    #[test]
    fn test_get_additional_headers() {
        let session_name = String::from("tiny-cow");
        let headers = HashMap::new();
        let add_headers = get_additional_headers(format!("https://tiny-cow.example.com/abc-xyz"), headers, &session_name);

        assert_eq!(add_headers.get("traceparent").unwrap().len(), 55);
        assert_eq!(add_headers.get("tracestate").unwrap(), "serpress-session=tiny-cow");
        assert_eq!(add_headers.get("X-Forwarded-Host").unwrap(), "example.com");

        let mut already_headers : HashMap<String, String> = HashMap::new();
        already_headers.insert(format!("traceparent"), format!("anything"));
        already_headers.insert(format!("tracestate"), format!("serpress-session=tiny-cow"));
        already_headers.insert(format!("X-Forwarded-Host"), format!("example.com"));
        let add_headers = get_additional_headers(format!("https://abc.some-tunnel.com/abc-xyz"), already_headers, &session_name);

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert!(add_headers.get("tracestate").is_none());

        let mut already_headers_two : HashMap<String, String> = HashMap::new();
        already_headers_two.insert(format!("traceparent"), format!("anything"));
        already_headers_two.insert(format!("tracestate"), format!("other-service=32"));
        already_headers_two.insert(format!("X-Forwarded-Host"), format!("example.com"));
        let add_headers = get_additional_headers(format!("https://abc.some-tunnel.com/abc-xyz"), already_headers_two, &session_name);

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert_eq!(add_headers.get("tracestate").unwrap(), "other-service=32,serpress-session=tiny-cow");
    }
}