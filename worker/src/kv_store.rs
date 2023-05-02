use linkup::{
    new_server_config, random_animal, random_six_char, server_config_to_yaml, NameKind,
    ServerConfig, SessionStore,
};
use worker::{console_log, kv::KvStore};

pub struct KvSessionStore {
    kv: KvStore,
}

impl KvSessionStore {
    pub fn new(kv: KvStore) -> Self {
        Self { kv }
    }
}

impl KvSessionStore {
    pub async fn get(&self, name: String) -> Option<ServerConfig> {
        console_log!("get: {}", name);
        let value = match self.kv.get(name.as_str()).text().await {
            Ok(Some(v)) => v,
            _ => return None,
        };

        console_log!("val: {}", value);
        Some(new_server_config(value).unwrap())
    }

    pub async fn new_session(
        &self,
        config: ServerConfig,
        name_kind: NameKind,
        desired_name: String,
    ) -> String {
        let name = self
            .choose_name(desired_name, config.session_token.clone(), name_kind)
            .await;
        let config_str = server_config_to_yaml(config);

        self.kv
            .put(&name, &config_str)
            .expect("unable to build kv put")
            .execute()
            .await
            .expect("Unable to store ServerConfig in KvStore");

        console_log!("put: {}", name);
        console_log!("val: {}", config_str);
        name
    }

    async fn choose_name(
        &self,
        desired_name: String,
        session_token: String,
        name_kind: NameKind,
    ) -> String {
        if let Some(session) = self.get(desired_name.clone()).await {
            if session.session_token == session_token {
                return desired_name;
            }
        }

        self.new_session_name(name_kind, desired_name).await
    }

    async fn exists(&self, name: String) -> bool {
        match self.kv.get(&name).text().await {
            Ok(Some(_)) => true,
            _ => false,
        }
    }

    async fn new_session_name(&self, name_kind: NameKind, desired_name: String) -> String {
        let mut key = String::new();

        if desired_name != "" {
            if !self.exists(desired_name.clone()).await {
                key = desired_name;
            }
        }

        if key.is_empty() {
            let mut tried_animal_key = false;
            loop {
                let generated_key = if !tried_animal_key && name_kind == NameKind::Animal {
                    tried_animal_key = true;
                    self.generate_unique_animal_key(20).await
                } else {
                    random_six_char()
                };

                if !self.exists(generated_key.clone()).await {
                    key = generated_key;
                    break;
                }
            }
        }

        key
    }

    async fn generate_unique_animal_key(&self, max_attempts: usize) -> String {
        for _ in 0..max_attempts {
            let generated_key = random_animal();
            if !self.exists(generated_key.clone()).await {
                return generated_key;
            }
        }
        // Fallback to SixChar logic
        random_six_char()
    }
}
