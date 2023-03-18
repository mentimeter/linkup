use std::{collections::HashMap, cell::RefCell};

use crate::name_gen::{new_session_name};
use crate::{ServerConfig, SessionStore, NameKind};

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