use crate::{
    ConfigError, HeaderMap, NameKind, Session, SessionError, StringStore,
    extract_tracestate_session, first_subdomain, headers::HeaderName, random_animal,
    random_six_char,
};

pub struct SessionAllocator<'a, S: StringStore> {
    store: &'a S,
}

impl<'a, S: StringStore> SessionAllocator<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    pub async fn get_request_session(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<(String, Session), SessionError> {
        let url_name = first_subdomain(url);
        if let Some(config) = self.get_session_config(&url_name).await? {
            return Ok((url_name, config));
        }

        if let Some(forwarded_host) = headers.get(HeaderName::ForwardedHost) {
            let forwarded_host_name = first_subdomain(forwarded_host);
            if let Some(config) = self.get_session_config(&forwarded_host_name).await? {
                return Ok((forwarded_host_name, config));
            }
        }

        if let Some(referer) = headers.get(HeaderName::Referer) {
            let referer_name = first_subdomain(referer);
            if let Some(config) = self.get_session_config(&referer_name).await? {
                return Ok((referer_name, config));
            }
        }

        if let Some(origin) = headers.get(HeaderName::Origin) {
            let origin_name = first_subdomain(origin);
            if let Some(config) = self.get_session_config(&origin_name).await? {
                return Ok((origin_name, config));
            }
        }

        if let Some(tracestate) = headers.get(HeaderName::TraceState) {
            let trace_name = extract_tracestate_session(tracestate);
            if let Some(config) = self.get_session_config(&trace_name).await? {
                return Ok((trace_name, config));
            }
        }

        Err(SessionError::NoSuchSession(url.to_string()))
    }

    pub async fn strict_store_session(
        &self,
        session_name: &str,
        session: &Session,
    ) -> Result<(), SessionError> {
        if session_name.is_empty() {
            return Err(SessionError::EmptySessionName);
        }

        if let Some(existing_session) = self.get_session_config(session_name).await?
            && existing_session.session_token != session.session_token
        {
            return Err(SessionError::SessionNameConflict);
        }

        let serialized_session = serde_json::to_string(&session)
            .map_err(|error| SessionError::ConfigErr(error.to_string()))?;

        self.store.put(session_name, &serialized_session).await?;

        Ok(())
    }

    // TODO(@augustoccesar)[2026-04-20]: Deprecate post 4.0 migration
    pub async fn store_session(
        &self,
        session: Session,
        name_kind: NameKind,
        desired_name: &str,
    ) -> Result<String, SessionError> {
        let name = self
            .choose_name(desired_name, &session.session_token, name_kind, &session)
            .await?;

        let serialized_session = serde_json::to_string(&session)
            .map_err(|error| SessionError::ConfigErr(error.to_string()))?;

        self.store.put(&name, &serialized_session).await?;

        Ok(name)
    }

    // TODO(@augustoccesar)[2026-04-20]: Deprecate post 4.0 migration
    async fn choose_name(
        &self,
        desired_name: &str,
        session_token: &str,
        name_kind: NameKind,
        session: &Session,
    ) -> Result<String, SessionError> {
        if !desired_name.is_empty()
            && let Some(session) = self.get_session_config(desired_name).await?
            && session.session_token == session_token
        {
            return Ok(desired_name.to_owned());
        }

        self.new_session_name(&name_kind, desired_name, session)
            .await
    }

    async fn get_session_config(&self, name: &str) -> Result<Option<Session>, SessionError> {
        let value = match self.store.get(name).await {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(e) => return Err(e),
        };

        let config_value: serde_json::Value =
            serde_json::from_str(&value).map_err(|e| SessionError::ConfigErr(e.to_string()))?;

        let session_config = config_value
            .try_into()
            .map_err(|e: ConfigError| SessionError::ConfigErr(e.to_string()))?;

        Ok(Some(session_config))
    }

    pub async fn new_session_name(
        &self,
        name_kind: &NameKind,
        desired_name: &str,
        session: &Session,
    ) -> Result<String, SessionError> {
        if name_kind == &NameKind::SixChar {
            return Ok(session.sha()[..6].to_string());
        }

        let mut key = String::new();

        if !desired_name.is_empty() && !self.store.exists(desired_name).await? {
            key = desired_name.to_owned();
        }

        if key.is_empty() {
            let mut tried_animal_key = false;
            loop {
                let generated_key = if !tried_animal_key {
                    tried_animal_key = true;
                    self.generate_unique_animal_key(20).await?
                } else {
                    random_six_char()
                };

                if !self.store.exists(&generated_key).await? {
                    key = generated_key;
                    break;
                }
            }
        }

        Ok(key)
    }

    async fn generate_unique_animal_key(
        &self,
        max_attempts: usize,
    ) -> Result<String, SessionError> {
        for _ in 0..max_attempts {
            let generated_key = random_animal();
            if !self.store.exists(&generated_key).await? {
                return Ok(generated_key);
            }
        }
        // Fallback to SixChar logic
        Ok(random_six_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryStringStore, UpsertSessionRequest};

    #[tokio::test]
    async fn identical_preview_requests_reuse_same_name() {
        let store = MemoryStringStore::default();
        let allocator = SessionAllocator::new(&store);
        let request_json = serde_json::json!({
            "services": [
                {
                    "name": "frontend",
                    "location": "https://frontend.example.com"
                },
                {
                    "name": "backend",
                    "location": "https://backend.example.com"
                }
            ],
            "domains": [
                {
                    "domain": "example.com",
                    "default_service": "frontend",
                    "routes": [
                        {
                            "path": "^/api(?:/|$)",
                            "service": "backend"
                        }
                    ]
                }
            ],
            "cache_routes": null
        })
        .to_string();

        let first_session =
            Session::try_from(serde_json::from_str::<UpsertSessionRequest>(&request_json).unwrap())
                .unwrap();

        let mut second_session = first_session.clone();
        second_session.services.reverse();

        let first_name = allocator
            .store_session(first_session, NameKind::SixChar, "")
            .await
            .unwrap();
        let second_name = allocator
            .store_session(second_session, NameKind::SixChar, "")
            .await
            .unwrap();

        assert_eq!(first_name.len(), 6);
        assert_eq!(first_name, second_name);
    }
}
