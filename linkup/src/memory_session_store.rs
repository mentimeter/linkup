use crate::{SessionError, StringStore};

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Default, Clone)]
pub struct MemoryStringStore(Arc<RwLock<HashMap<String, String>>>);

impl StringStore for MemoryStringStore {
    async fn get(&self, key: &str) -> Result<Option<String>, SessionError> {
        match self.0.read() {
            Ok(l) => Ok(l.get(key).cloned()),
            Err(e) => Err(SessionError::GetError(e.to_string())),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, SessionError> {
        let value = match self.0.read() {
            Ok(l) => Ok(l.get(key).cloned()),
            Err(e) => return Err(SessionError::GetError(e.to_string())),
        }?;

        match value {
            Some(_) => Ok(true),
            _ => Ok(false),
        }
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), SessionError> {
        match self.0.write() {
            Ok(mut l) => Ok(l.insert(key.to_owned(), value.to_owned())),
            Err(e) => Err(SessionError::PutError(e.to_string())),
        }?;

        Ok(())
    }
}
