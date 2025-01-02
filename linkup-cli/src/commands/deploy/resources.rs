use serde::{Deserialize, Serialize};

use super::{api::CloudflareApi, cf_deploy::DeployNotifier, DeployError};

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
        format!("{}{}/*", self.route, zone_name)
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

/// A plan describing all actions that need to be taken to get Cloudflare
/// resources to match the desired state.
#[derive(Debug, Default)]
pub struct DeployPlan {
    pub kv_action: Option<KvPlan>,
    pub script_action: Option<WorkerScriptPlan>,
    pub dns_actions: Vec<DnsRecordPlan>,
    pub route_actions: Vec<WorkerRoutePlan>,
    pub ruleset_actions: Vec<RulesetPlan>,
}

/// Plan describing how to reconcile the KV namespace.
#[derive(Debug)]
pub enum KvPlan {
    /// Create the KV namespace with the given name.
    Create { namespace_name: String },
}

/// Plan describing how to reconcile a worker script.
#[derive(Debug)]
pub enum WorkerScriptPlan {
    /// Upload the script with the given metadata & parts
    Upload {
        script_name: String,
        metadata: WorkerMetadata,
        parts: Vec<WorkerScriptPart>,
    },
}

/// Plan describing how to reconcile a DNS record.
#[derive(Debug)]
pub enum DnsRecordPlan {
    /// Create the DNS record in a particular zone.
    Create { zone_id: String, record: DNSRecord },
}

/// Plan describing how to reconcile a worker route.
#[derive(Debug)]
pub enum WorkerRoutePlan {
    /// Create the worker route in a particular zone.
    Create {
        zone_id: String,
        pattern: String,
        script_name: String,
    },
}

/// Plan describing how to reconcile the ruleset.
#[derive(Debug)]
pub struct RulesetPlan {
    pub zone_id: String,
    pub ruleset_id: Option<String>,
    /// True if we need to create a new ruleset, false if we only need to update existing.
    pub create_new: bool,
    pub rules: Vec<Rule>,
}

const LINKUP_SCRIPT_NAME: &str = "linkup-worker";
const LINKUP_WORKER_SHIM: &[u8] = include_bytes!("../../../../worker/build/worker/shim.mjs");
const LINKUP_WORKER_INDEX_WASM: &[u8] =
    include_bytes!("../../../../worker/build/worker/index.wasm");

impl TargetCfResources {
    /// Collect all plan actions into a single DeployPlan.
    pub async fn check_deploy_plan(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<DeployPlan, DeployError> {
        let kv_action = self.check_kv_namespace(api).await?;
        let script_action = self.check_worker_script(api).await?;
        let dns_actions = self.check_dns_records(api).await?;
        let route_actions = self.check_worker_routes(api).await?;
        let ruleset_actions = self.check_rulesets(api).await?;

        Ok(DeployPlan {
            kv_action,
            script_action,
            dns_actions,
            route_actions,
            ruleset_actions,
        })
    }
    /// Check if the KV namespace exists and return the plan if not.
    pub async fn check_kv_namespace(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Option<KvPlan>, DeployError> {
        let existing = api.get_kv_namespace_id(self.kv_name.clone()).await?;
        if existing.is_none() {
            // We need to create this KV namespace
            Ok(Some(KvPlan::Create {
                namespace_name: self.kv_name.clone(),
            }))
        } else {
            // KV namespace is already present, no plan needed
            Ok(None)
        }
    }

    /// Check if the worker script needs to be uploaded. Return the plan if an upload is needed.
    pub async fn check_worker_script(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Option<WorkerScriptPlan>, DeployError> {
        let script_name = &self.worker_script_name;
        let existing_info = api.get_worker_script_info(script_name.clone()).await?;

        // Decide if we need to upload:
        let needs_upload = if let Some(_) = existing_info {
            let existing_content = api.get_worker_script_content(script_name.clone()).await?;
            // For simplicity, we just compare to some known "marker"
            existing_content != "TODO: some other string"
        } else {
            true
        };

        if needs_upload {
            // Construct the metadata
            let metadata = WorkerMetadata {
                main_module: self.worker_script_entry.clone(),
                bindings: vec![WorkerKVBinding {
                    type_: "kv_namespace".to_string(),
                    name: "LINKUP_SESSIONS".to_string(),
                    namespace_id: "<to-be-filled-on-deploy>".to_string(),
                }],
                compatibility_date: "2024-12-18".to_string(),
            };
            Ok(Some(WorkerScriptPlan::Upload {
                script_name: script_name.clone(),
                metadata,
                parts: self.worker_script_parts.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Check if we need to create DNS records for each zone.
    /// We return one plan item per missing DNS record.
    pub async fn check_dns_records(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Vec<DnsRecordPlan>, DeployError> {
        let mut plans = Vec::new();

        for zone_id in api.zone_ids() {
            for dns_record in &self.zone_resources.dns_records {
                let record_tag = dns_record.comment();
                let existing_dns = api.get_dns_record(zone_id.clone(), record_tag).await?;
                if existing_dns.is_none() {
                    // We need to create a new record
                    // We'll fill the content with a placeholder, as the actual subdomain
                    // might come from checking the worker subdomain.
                    let record = DNSRecord {
                        id: "".to_string(),
                        name: dns_record.route.clone(),
                        record_type: "CNAME".to_string(),
                        content: "will-be-filled-later".to_string(),
                        comment: dns_record.comment(),
                        proxied: true,
                    };
                    plans.push(DnsRecordPlan::Create {
                        zone_id: zone_id.clone(),
                        record,
                    });
                }
            }
        }
        Ok(plans)
    }

    /// Check if we need to create worker routes for each zone.
    pub async fn check_worker_routes(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Vec<WorkerRoutePlan>, DeployError> {
        let mut plans = Vec::new();

        for zone_id in api.zone_ids() {
            let zone_name = api.get_zone_name(zone_id.clone()).await?;
            for route_config in &self.zone_resources.routes {
                let route_pattern = route_config.worker_route(zone_name.clone());
                let script_name = route_config.script.clone();
                let existing_route = api
                    .get_worker_route(zone_id.clone(), route_pattern.clone(), script_name.clone())
                    .await?;

                if existing_route.is_none() {
                    plans.push(WorkerRoutePlan::Create {
                        zone_id: zone_id.clone(),
                        pattern: route_pattern,
                        script_name,
                    });
                }
            }
        }
        Ok(plans)
    }

    /// Check if we need to create/update the cache ruleset for each zone.
    pub async fn check_rulesets(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Vec<RulesetPlan>, DeployError> {
        let mut plans = Vec::new();
        let ruleset_name = self.zone_resources.cache_rules.name.clone();
        let ruleset_phase = self.zone_resources.cache_rules.phase.clone();
        let desired_rules = self.zone_resources.cache_rules.rules.clone();

        for zone_id in api.zone_ids() {
            let existing_ruleset_id = api
                .get_ruleset(zone_id.clone(), ruleset_name.clone(), ruleset_phase.clone())
                .await?;

            if let Some(ruleset_id) = existing_ruleset_id {
                // We have a ruleset. Check if the rules are the same.
                let current_rules = api
                    .get_ruleset_rules(zone_id.clone(), ruleset_id.clone())
                    .await?;
                if !rules_equal(&current_rules, &desired_rules) {
                    plans.push(RulesetPlan {
                        zone_id: zone_id.clone(),
                        ruleset_id: Some(ruleset_id),
                        create_new: false,
                        rules: desired_rules.clone(),
                    });
                }
            } else {
                // We'll need to create a new ruleset
                plans.push(RulesetPlan {
                    zone_id: zone_id.clone(),
                    ruleset_id: None,
                    create_new: true,
                    rules: desired_rules.clone(),
                });
            }
        }
        Ok(plans)
    }

    pub async fn execute_deploy_plan(
        &self,
        api: &impl CloudflareApi,
        plan: &DeployPlan,
        notifier: &impl DeployNotifier,
    ) -> Result<(), DeployError> {
        // We may need the worker subdomain for DNS records, so fetch it once:
        let worker_subdomain = api.get_worker_subdomain().await?;

        // 1) Reconcile KV
        if let Some(KvPlan::Create { namespace_name }) = &plan.kv_action {
            notifier.notify(&format!("Creating KV namespace: {}", namespace_name));
            let new_id = api.create_kv_namespace(namespace_name.clone()).await?;
            notifier.notify(&format!("KV namespace created with ID: {}", new_id));
        }

        // 2) Reconcile Worker Script
        //    We may need the KV namespace ID we just created, so we fetch it again here.
        if let Some(WorkerScriptPlan::Upload {
            script_name,
            metadata,
            parts,
        }) = &plan.script_action
        {
            let kv_ns_id = api.get_kv_namespace_id(self.kv_name.clone()).await?;
            let kv_ns_id = kv_ns_id.ok_or_else(|| {
                DeployError::UnexpectedResponse(
                    "KV namespace should exist but was not found".to_string(),
                )
            })?;

            // Update metadata with correct ID
            let mut final_metadata = metadata.clone();
            if let Some(binding) = final_metadata
                .bindings
                .iter_mut()
                .find(|b| b.name == "LINKUP_SESSIONS")
            {
                binding.namespace_id = kv_ns_id;
            }

            notifier.notify("Uploading worker script...");
            api.create_worker_script(script_name.clone(), final_metadata, parts.clone())
                .await?;
            notifier.notify("Worker script uploaded successfully.");
        }

        // 3) Reconcile DNS records
        //    We only have a plan for *missing* records, so each DnsRecordPlan must be created.
        for dns_plan in &plan.dns_actions {
            if let DnsRecordPlan::Create { zone_id, record } = dns_plan {
                let final_record = {
                    let mut r = record.clone();
                    // Fill in the correct content from the subdomain (if any)
                    let cname_target = if let Some(sub) = &worker_subdomain {
                        format!("{}.{}.workers.dev", self.worker_script_name, sub)
                    } else {
                        format!("{}.workers.dev", self.worker_script_name)
                    };
                    r.content = cname_target;
                    r
                };
                notifier.notify(&format!(
                    "Creating DNS record '{}' in zone {} -> {}",
                    final_record.name, zone_id, final_record.content
                ));
                api.create_dns_record(zone_id.clone(), final_record).await?;
            }
        }

        // 4) Reconcile Worker Routes
        for route_plan in &plan.route_actions {
            if let WorkerRoutePlan::Create {
                zone_id,
                pattern,
                script_name,
            } = route_plan
            {
                notifier.notify(&format!(
                    "Creating route '{}' in zone {} -> script '{}'",
                    pattern, zone_id, script_name
                ));
                api.create_worker_route(zone_id.clone(), pattern.clone(), script_name.clone())
                    .await?;
            }
        }

        // 5) Reconcile Cache Rulesets
        for ruleset_plan in &plan.ruleset_actions {
            let RulesetPlan {
                zone_id,
                ruleset_id,
                create_new,
                rules,
            } = ruleset_plan;

            // If we have to create a new ruleset
            let final_id = if *create_new {
                notifier.notify(&format!("Creating new cache ruleset in zone '{}'", zone_id));
                let new_id = api
                    .create_ruleset(
                        zone_id.clone(),
                        self.zone_resources.cache_rules.name.clone(),
                        self.zone_resources.cache_rules.phase.clone(),
                    )
                    .await?;
                notifier.notify(&format!(
                    "Ruleset created with ID: {} in zone {}",
                    new_id, zone_id
                ));
                new_id
            } else {
                // We already have an ID
                ruleset_id.clone().unwrap()
            };

            notifier.notify(&format!(
                "Updating cache ruleset '{}' with new rules in zone {}",
                final_id, zone_id
            ));
            api.update_ruleset_rules(zone_id.clone(), final_id, rules.clone())
                .await?;
        }

        Ok(())
    }
}

impl DeployPlan {
    /// Return `true` if there are no actions to execute.
    pub fn is_empty(&self) -> bool {
        self.kv_action.is_none()
            && self.script_action.is_none()
            && self.dns_actions.is_empty()
            && self.route_actions.is_empty()
            && self.ruleset_actions.is_empty()
    }
}

/// Compare rules by their relevant fields only.
pub fn rules_equal(current: &Vec<Rule>, desired: &Vec<Rule>) -> bool {
    if current.len() != desired.len() {
        return false;
    }
    for (c, d) in current.iter().zip(desired.iter()) {
        if c.action != d.action
            || c.description != d.description
            || c.enabled != d.enabled
            || c.expression != d.expression
            || c.action_parameters != d.action_parameters
        {
            return false;
        }
    }
    true
}

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
