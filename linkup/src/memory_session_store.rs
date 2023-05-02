use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;

use crate::{SessionError, StringStore};

pub struct MemoryStringStore {
    store: Mutex<HashMap<String, String>>,
}

impl MemoryStringStore {
    pub fn new() -> Self {
        MemoryStringStore {
            store: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait(?Send)]
impl StringStore for MemoryStringStore {
    async fn get(&self, key: String) -> Result<Option<String>, SessionError> {
        match self.store.lock() {
            Ok(l) => Ok(l.get(key.as_str()).cloned()),
            Err(e) => Err(SessionError::GetError(e.to_string())),
        }
    }

    async fn exists(&self, key: String) -> Result<bool, SessionError> {
        let value = match self.store.lock() {
            Ok(l) => Ok(l.get(&key).cloned()),
            Err(e) => return Err(SessionError::GetError(e.to_string())),
        }?;

        match value {
            Some(_) => Ok(true),
            _ => Ok(false),
        }
    }

    async fn put(&self, key: String, value: String) -> Result<(), SessionError> {
        match self.store.lock() {
            Ok(mut l) => Ok(l.insert(key, value)),
            Err(e) => Err(SessionError::PutError(e.to_string())),
        }?;

        Ok(())
    }
}
