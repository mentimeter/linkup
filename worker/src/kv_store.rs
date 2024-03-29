use linkup::{SessionError, StringStore};
use worker::kv::KvStore;

pub struct CfWorkerStringStore {
    kv: KvStore,
}

impl CfWorkerStringStore {
    pub fn new(kv: KvStore) -> Self {
        Self { kv }
    }
}

impl StringStore for CfWorkerStringStore {
    async fn get(&self, key: String) -> Result<Option<String>, SessionError> {
        match self.kv.get(key.as_str()).text().await {
            Ok(v) => Ok(v),
            Err(e) => Err(SessionError::GetError(e.to_string())),
        }
    }

    async fn exists(&self, key: String) -> Result<bool, SessionError> {
        let value = match self.kv.get(key.as_str()).text().await {
            Ok(v) => Ok(v),
            Err(e) => return Err(SessionError::GetError(e.to_string())),
        }?;

        match value {
            Some(_) => Ok(true),
            _ => Ok(false),
        }
    }

    async fn put(&self, key: String, value: String) -> Result<(), SessionError> {
        let mut put = match self.kv.put(&key, value) {
            Ok(p) => p,
            Err(e) => return Err(SessionError::PutError(e.to_string())),
        };

        // Default to expiring sessions after 7 days of inactivity
        put = put.expiration_ttl(60 * 60 * 24 * 7);

        put.execute()
            .await
            .map_err(|e| SessionError::PutError(e.to_string()))
    }
}
