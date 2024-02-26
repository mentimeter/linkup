use crate::{
    extract_tracestate_session, first_subdomain, headers::HeaderName,
    name_gen::deterministic_six_char_hash, random_animal, random_six_char, session_to_json,
    ConfigError, HeaderMap, NameKind, Session, SessionError, StringStore,
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
        if let Some(config) = self.get_session_config(url_name.to_string()).await? {
            return Ok((url_name, config));
        }

        if let Some(forwarded_host) = headers.get(HeaderName::ForwardedHost) {
            let forwarded_host_name = first_subdomain(forwarded_host);
            if let Some(config) = self
                .get_session_config(forwarded_host_name.to_string())
                .await?
            {
                return Ok((forwarded_host_name, config));
            }
        }

        if let Some(referer) = headers.get(HeaderName::Referer) {
            let referer_name = first_subdomain(referer);
            if let Some(config) = self.get_session_config(referer_name.to_string()).await? {
                return Ok((referer_name, config));
            }
        }

        if let Some(origin) = headers.get(HeaderName::Origin) {
            let origin_name = first_subdomain(origin);
            if let Some(config) = self.get_session_config(origin_name.to_string()).await? {
                return Ok((origin_name, config));
            }
        }

        if let Some(tracestate) = headers.get(HeaderName::TraceState) {
            let trace_name = extract_tracestate_session(tracestate);
            if let Some(config) = self.get_session_config(trace_name.to_string()).await? {
                return Ok((trace_name, config));
            }
        }

        if let Some(baggage) = headers.get(HeaderName::Baggage) {
            let baggage_name = extract_tracestate_session(baggage);
            if let Some(config) = self.get_session_config(baggage_name.to_string()).await? {
                return Ok((baggage_name, config));
            }
        }

        Err(SessionError::NoSuchSession(url.to_string()))
    }

    pub async fn store_session(
        &self,
        config: Session,
        name_kind: NameKind,
        desired_name: String,
    ) -> Result<String, SessionError> {
        let config_str = session_to_json(config.clone());

        let name = self
            .choose_name(desired_name, config.session_token, name_kind, &config_str)
            .await?;

        self.store.put(name.clone(), config_str).await?;

        Ok(name)
    }

    async fn choose_name(
        &self,
        desired_name: String,
        session_token: String,
        name_kind: NameKind,
        config_json: &str,
    ) -> Result<String, SessionError> {
        if desired_name.is_empty() {
            return self
                .new_session_name(name_kind, desired_name, config_json)
                .await;
        }

        if let Some(session) = self.get_session_config(desired_name.clone()).await? {
            if session.session_token == session_token {
                return Ok(desired_name);
            }
        }

        self.new_session_name(name_kind, desired_name, config_json)
            .await
    }

    async fn get_session_config(&self, name: String) -> Result<Option<Session>, SessionError> {
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

    async fn new_session_name(
        &self,
        name_kind: NameKind,
        desired_name: String,
        config_json: &str,
    ) -> Result<String, SessionError> {
        if name_kind == NameKind::SixChar {
            return Ok(deterministic_six_char_hash(config_json));
        }

        let mut key = String::new();

        if !desired_name.is_empty() && !self.store.exists(desired_name.clone()).await? {
            key = desired_name;
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

                if !self.store.exists(generated_key.clone()).await? {
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
            if !self.store.exists(generated_key.clone()).await? {
                return Ok(generated_key);
            }
        }
        // Fallback to SixChar logic
        Ok(random_six_char())
    }
}
