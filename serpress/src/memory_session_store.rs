use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::name_gen::new_session_name;
use crate::{NameKind, ServerConfig, SessionStore};

pub struct MemorySessionStore {
    store: Mutex<HashMap<String, ServerConfig>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        MemorySessionStore {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl SessionStore for MemorySessionStore {
    fn get(&self, name: &String) -> Option<ServerConfig> {
        match self.store.lock() {
            Ok(l) => l.get(name).cloned(),
            Err(_) => None,
        }
    }

    fn new(
        &self,
        config: ServerConfig,
        name_kind: NameKind,
        desired_name: Option<String>,
    ) -> String {
        let exists_fn = |name: String| match self.store.lock() {
            Ok(l) => l.contains_key(&name),
            Err(_) => false,
        };
        let key = new_session_name(name_kind, desired_name, &exists_fn);
        self.store.lock().unwrap().insert(key.clone(), config);
        key
    }
}
