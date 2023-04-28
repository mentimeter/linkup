use linkup::{new_server_config, server_config_to_yaml, NameKind, ServerConfig, SessionStore, random_animal, random_six_char};
use worker::{kv::KvStore};


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
        let value = match self.kv.get(name.as_str()).text().await {
            Ok(Some(v)) => v,
            _ => return None,
        };
        Some(new_server_config(value).unwrap())
    }

    pub async fn new_session(
        &self,
        config: ServerConfig,
        name_kind: NameKind,
        desired_name: Option<String>,
    ) -> String {

        let new_name = self.new_session_name(name_kind, desired_name).await;
        let config_str = server_config_to_yaml(config);


        self.kv
            .put(&new_name, &config_str)
            .expect("unable to build kv put")
            .execute().await.expect("Unable to store ServerConfig in KvStore");

        new_name
    }

    async fn exists(&self, name: String) -> bool {
        match self.kv.get(&name).text().await {
            Ok(Some(_)) => true,
            _ => false,
        }
    }


    async fn new_session_name(
        &self,
        name_kind: NameKind,
        desired_name: Option<String>,
    ) -> String {
        let mut key = String::new();
    
        if let Some(name) = desired_name {
            if !self.exists(name.clone()).await {
                key = name;
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
