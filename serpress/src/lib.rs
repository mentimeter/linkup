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

pub fn get_request_session<T: SessionStore>(domain: String, headers: HashMap<String, String>, store: &T) -> Result<ServerConfig, SessionError> {
    Err(SessionError::NoSuchSession(domain))
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

        get_request_session(format!("{}.example.com", name), HashMap::new(), &session_store).unwrap();
    }
}