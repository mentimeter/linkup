use futures::executor::block_on;
use serpress::{new_server_config, server_config_to_yaml, NameKind, ServerConfig, SessionStore};
use worker::{kv::KvStore, *};

pub struct KvSessionStore {
    kv: KvStore,
}

impl KvSessionStore {
    pub fn new() -> Self {
        let kv = KvStore::create("SERPRESS_SESSIONS").expect("Unable to initialize KvStore");
        Self { kv }
    }
}

impl SessionStore for KvSessionStore {
    fn get(&self, name: &String) -> Option<ServerConfig> {
        let value = match block_on(self.kv.get(name).text()) {
            Ok(Some(v)) => v,
            _ => return None,
        };
        Some(new_server_config(value).unwrap())
    }

    fn new(
        &self,
        config: ServerConfig,
        name_kind: NameKind,
        desired_name: Option<String>,
    ) -> String {
        let exists_fn = |name: String| match block_on(self.kv.get(&name).text()) {
            Ok(Some(_)) => true,
            _ => false,
        };

        let new_name = serpress::new_session_name(name_kind, desired_name, &exists_fn);
        let config_str = server_config_to_yaml(config);

        block_on(
            self.kv
                .put(&new_name, &config_str)
                .expect("unable to build kv put")
                .execute(),
        )
        .expect("Unable to store ServerConfig in KvStore");
        new_name
    }
}
