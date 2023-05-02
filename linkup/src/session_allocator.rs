use std::{collections::HashMap, sync::Arc};

use crate::{
    extract_tracestate_session, first_subdomain, new_server_config, server_config_to_yaml,
    NameKind, ServerConfig, SessionError, StringStore, random_six_char, random_animal,
};

pub struct SessionAllocator {
    store: Arc<dyn StringStore>,
}

impl SessionAllocator {
    pub fn new(store: Arc<dyn StringStore>) -> Self {
        Self { store }
    }

    pub async fn get_request_session(
        &self,
        url: String,
        headers: HashMap<String, String>,
    ) -> Result<(String, ServerConfig), SessionError> {
        let url_name = first_subdomain(&url);
        if let Some(config) = self.get_session_config(url_name.to_string()).await? {
            return Ok((url_name, config));
        }

        if let Some(referer) = headers.get("referer") {
            let referer_name = first_subdomain(referer);
            if let Some(config) = self.get_session_config(referer_name.to_string()).await? {
                return Ok((referer_name, config));
            }
        }

        if let Some(tracestate) = headers.get("tracestate") {
            let trace_name = extract_tracestate_session(tracestate);
            if let Some(config) = self.get_session_config(trace_name.to_string()).await? {
                return Ok((trace_name, config));
            }
        }

        Err(SessionError::NoSuchSession(url))
    }

    pub async fn store_session(
        &self,
        config: ServerConfig,
        name_kind: NameKind,
        desired_name: String,
    ) -> Result<String, SessionError> {
        let name = self
            .choose_name(desired_name, config.session_token.clone(), name_kind)
            .await?;
        let config_str = server_config_to_yaml(config);

        self.store.put(name.clone(), config_str).await?;

        Ok(name)
    }

    async fn choose_name(
        &self,
        desired_name: String,
        session_token: String,
        name_kind: NameKind,
    ) -> Result<String, SessionError> {
        if desired_name == "" {
            return self.new_session_name(name_kind, desired_name).await;
        }

        if let Some(session) = self.get_session_config(desired_name.clone()).await? {
            if session.session_token == session_token {
                return Ok(desired_name);
            }
        }

        self.new_session_name(name_kind, desired_name).await
    }

    async fn get_session_config(&self, name: String) -> Result<Option<ServerConfig>, SessionError> {
        let value = match self.store.get(name).await {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(e) => return Err(e),
        };

        let config =
            new_server_config(value).map_err(|e| SessionError::ConfigErr(e.to_string()))?;
        Ok(Some(config))
    }

    async fn new_session_name(&self, name_kind: NameKind, desired_name: String) -> Result<String , SessionError> {
        let mut key = String::new();

        if desired_name != "" {
            if !self.store.exists(desired_name.clone()).await? {
                key = desired_name;
            }
        }

        if key.is_empty() {
            let mut tried_animal_key = false;
            loop {
                let generated_key = if !tried_animal_key && name_kind == NameKind::Animal {
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

    async fn generate_unique_animal_key(&self, max_attempts: usize) -> Result<String, SessionError> {
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
