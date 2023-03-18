use std::collections::HashMap;
use std::cell::RefCell;
use thiserror::Error;

mod server_config;
mod name_gen;

use name_gen::{new_session_name};
pub use server_config::*;

pub trait SessionStore {
    fn get(&self, name: String) -> Option<ServerConfig>;
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
    fn get(&self, name: String) -> Option<ServerConfig> {
        self.store.borrow().get(&name).cloned()
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

pub fn get_request_session<T: SessionStore>(url: String, headers: HashMap<String, String>, store: &T) -> Result<ServerConfig, SessionError> {
    if let Some(config) = store.get(first_subdomain(&url)) {
        return Ok(config);
    }

    if let Some(referer) = headers.get("referer") {
        if let Some(config) = store.get(first_subdomain(referer)) {
            return Ok(config);
        }
    }

    if let Some(tracestate) = headers.get("tracestate") {
        if let Some(config) = store.get(extract_tracestate(tracestate)) {
            return Ok(config);
        }
    }

    Err(SessionError::NoSuchSession(url))
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
}