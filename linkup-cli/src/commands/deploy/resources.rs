use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct TargetCfResources {
    pub worker_script_name: String,
    pub worker_script_parts: Vec<WorkerScriptPart>,
    pub worker_script_entry: String,
    pub kv_name: String,
    pub zone_resources: TargectCfZoneResources,
}

#[derive(Debug, Clone)]
pub struct TargectCfZoneResources {
    pub dns_records: Vec<TargetDNSRecord>,
    pub routes: Vec<TargetWorkerRoute>,
    pub cache_rules: TargetCacheRules,
}

#[derive(Debug, Clone)]
pub struct TargetDNSRecord {
    pub route: String,
    pub script: String,
}

impl TargetDNSRecord {
    pub fn comment(&self) -> String {
        format!("{}-{}", self.script, self.route)
    }

    pub fn target(&self, worker_subdomain: Option<String>) -> String {
        if let Some(sub) = worker_subdomain {
            format!("{}.{}.workers.dev", self.script, sub)
        } else {
            format!("{}.workers.dev", self.script)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TargetWorkerRoute {
    pub route: String,
    pub script: String,
}

impl TargetWorkerRoute {
    pub fn worker_route(&self, zone_name: String) -> String {
        format!("{}.{}/*", self.route, zone_name)
    }
}

#[derive(Debug, Clone)]
pub struct TargetCacheRules {
    pub name: String,
    pub phase: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct WorkerScriptInfo {
    pub id: String,
}

pub struct WorkerMetadata {
    pub main_module: String,
    pub bindings: Vec<WorkerKVBinding>,
    pub compatibility_date: String,
}

#[derive(Debug, Clone)]
pub struct WorkerScriptPart {
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub struct WorkerKVBinding {
    pub type_: String,
    pub name: String,
    pub namespace_id: String,
}

#[derive(Debug, Clone)]
pub struct DNSRecord {
    pub id: String,
    pub name: String,
    pub record_type: String,
    pub content: String,
    pub comment: String,
    pub proxied: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Rule {
    pub action: String,
    pub description: String,
    pub enabled: bool,
    pub expression: String,
    pub action_parameters: Option<serde_json::Value>,
}

const LINKUP_SCRIPT_NAME: &str = "linkup-worker";
const LINKUP_WORKER_SHIM: &[u8] = include_bytes!("../../../../worker/build/worker/shim.mjs");
const LINKUP_WORKER_INDEX_WASM: &[u8] =
    include_bytes!("../../../../worker/build/worker/index.wasm");

pub fn cf_resources() -> TargetCfResources {
    TargetCfResources {
        worker_script_name: LINKUP_SCRIPT_NAME.to_string(),
        worker_script_entry: "shim.mjs".to_string(),
        worker_script_parts: vec![
            WorkerScriptPart {
                name: "shim.mjs".to_string(),
                data: LINKUP_WORKER_SHIM.to_vec(),
                content_type: "application/javascript+module".to_string(),
            },
            WorkerScriptPart {
                name: "index.wasm".to_string(),
                data: LINKUP_WORKER_INDEX_WASM.to_vec(),
                content_type: "application/wasm".to_string(),
            },
        ],
        kv_name: "linkup-session-kv".to_string(),
        zone_resources: TargectCfZoneResources {
            dns_records: vec![
                TargetDNSRecord {
                    route: "*".to_string(),
                    script: LINKUP_SCRIPT_NAME.to_string(),
                },
                TargetDNSRecord {
                    route: "@".to_string(),
                    script: LINKUP_SCRIPT_NAME.to_string(),
                },
            ],
            routes: vec![
                TargetWorkerRoute {
                    route: "*.".to_string(),
                    script: LINKUP_SCRIPT_NAME.to_string(),
                },
                TargetWorkerRoute {
                    route: "".to_string(),
                    script: LINKUP_SCRIPT_NAME.to_string(),
                },
            ],
            cache_rules: TargetCacheRules {
                name: "default".to_string(),
                phase: "http_request_cache_settings".to_string(),
                rules: vec![Rule {
                    action: "set_cache_settings".to_string(),
                    description: "linkup cache rule - do not cache tunnel requests".to_string(),
                    enabled: true,
                    expression: "(starts_with(http.host, \"tunnel-\"))".to_string(),
                    action_parameters: Some(serde_json::json!({"cache": false})),
                }],
            },
        },
    }
}
