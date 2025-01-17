use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{api::CloudflareApi, cf_deploy::DeployNotifier, DeployError};

const LINKUP_ACCOUNT_TOKEN_NAME: &str = "linkup-account-owned-cli-access-token";
const LINKUP_SCRIPT_NAME: &str = "linkup-worker";
// To build the worker script, run in the worker directory:
// cargo install -q worker-build && worker-build --release
const LINKUP_WORKER_SHIM: &[u8] = include_bytes!("../../../../worker/build/worker/shim.mjs");
const LINKUP_WORKER_INDEX_WASM: &[u8] =
    include_bytes!("../../../../worker/build/worker/index.wasm");

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
pub struct WorkerScriptInfo {}

#[derive(Debug, Clone)]
pub struct WorkerMetadata {
    pub main_module: String,
    pub bindings: Vec<WorkerKVBinding>,
    pub compatibility_date: String,
    pub tag: String,
}

#[derive(Clone)]
pub struct WorkerScriptPart {
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

impl fmt::Debug for WorkerScriptPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerScriptPart")
            .field("name", &self.name)
            .field("content_type", &self.content_type)
            .finish()
    }
}

impl fmt::Display for WorkerScriptPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WorkerScriptPart {{ name: {}, content_type: {} }}",
            self.name, self.content_type
        )
    }
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
    pub account_token_action: Option<AccountTokenPlan>,
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

#[derive(Debug)]
pub struct AccountTokenPlan {
    pub token_name: String,
}

#[derive(Debug, Default)]
pub struct DestroyPlan {
    /// Name of worker script that should be removed (if any).
    pub remove_worker_script: Option<String>,

    /// ID of the KV namespace that should be removed (if any).
    pub remove_kv_namespace: Option<String>,

    /// (zone_id, dns_record_id) for each DNS record to remove.
    pub remove_dns_records: Vec<(String, String)>,

    /// (zone_id, route_id) for each worker route to remove.
    pub remove_worker_routes: Vec<(String, String)>,

    /// (zone_id, ruleset_id) for each cache ruleset to remove.
    pub remove_rulesets: Vec<(String, String)>,

    /// Name token that should be removed (if any).
    pub remove_account_token: Option<String>,
}

impl DestroyPlan {
    /// Return true if nothing needs to be removed.
    pub fn is_empty(&self) -> bool {
        self.remove_worker_script.is_none()
            && self.remove_kv_namespace.is_none()
            && self.remove_dns_records.is_empty()
            && self.remove_worker_routes.is_empty()
            && self.remove_rulesets.is_empty()
            && self.remove_account_token.is_none()
    }
}

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
        let account_token_action = self.check_account_token(api).await?;

        Ok(DeployPlan {
            kv_action,
            script_action,
            dns_actions,
            route_actions,
            ruleset_actions,
            account_token_action,
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
        let last_version = api.get_worker_script_version(script_name.clone()).await?;
        let current_version = self.worker_version_hash();

        // Decide if we need to upload:
        let needs_upload = if let Some(last_version) = last_version {
            last_version != current_version
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
                tag: current_version,
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

    pub async fn check_account_token(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Option<AccountTokenPlan>, DeployError> {
        let tokens = api.list_account_tokens().await?;
        let linkup_token = tokens
            .iter()
            .find(|token| token.name.as_deref() == Some(LINKUP_ACCOUNT_TOKEN_NAME));

        match linkup_token {
            Some(_) => Ok(None),
            None => Ok(Some(AccountTokenPlan {
                token_name: LINKUP_ACCOUNT_TOKEN_NAME.to_string(),
            })),
        }
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
            let DnsRecordPlan::Create { zone_id, record } = dns_plan;
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

        // 4) Reconcile Worker Routes
        for route_plan in &plan.route_actions {
            let WorkerRoutePlan::Create {
                zone_id,
                pattern,
                script_name,
            } = route_plan;
            notifier.notify(&format!(
                "Creating route '{}' in zone {} -> script '{}'",
                pattern, zone_id, script_name
            ));
            api.create_worker_route(zone_id.clone(), pattern.clone(), script_name.clone())
                .await?;
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

        // 6) Reconcile account token
        if let Some(AccountTokenPlan { token_name }) = &plan.account_token_action {
            notifier.notify("Creating account token...");
            let token = api.create_account_token(token_name).await?;
            notifier.notify("Account token created successfully");
            notifier.notify(
                "-----------------------------------------------------------------------------",
            );
            notifier.notify(
                "-------------------------------- NOTICE -------------------------------------",
            );
            notifier.notify(
                "This is the only time you'll get to see this token, make sure to make a copy.",
            );
            notifier.notify(&format!("Access token: {}", token));
        }

        Ok(())
    }

    /// Gather all the resources that actually exist and need to be removed.
    pub async fn check_destroy_plan(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<DestroyPlan, DeployError> {
        let mut plan = DestroyPlan::default();

        // 1) Worker script
        let script_name = &self.worker_script_name;
        let existing_info = api.get_worker_script_info(script_name.clone()).await?;
        if existing_info.is_some() {
            plan.remove_worker_script = Some(script_name.clone());
        }

        // 2) KV namespace
        let kv_name = &self.kv_name;
        if let Some(ns_id) = api.get_kv_namespace_id(kv_name.clone()).await? {
            plan.remove_kv_namespace = Some(ns_id);
        }

        // 3) DNS records
        for zone_id in api.zone_ids() {
            for dns_record in &self.zone_resources.dns_records {
                let record_tag = dns_record.comment();
                if let Some(existing_dns) = api.get_dns_record(zone_id.clone(), record_tag).await? {
                    // We store (zone_id, record_id) so we know exactly which to remove
                    plan.remove_dns_records
                        .push((zone_id.clone(), existing_dns.id));
                }
            }
        }

        // 4) Worker routes
        for zone_id in api.zone_ids() {
            let zone_name = api.get_zone_name(zone_id.clone()).await?;
            for route_config in &self.zone_resources.routes {
                let pattern = route_config.worker_route(zone_name.clone());
                let script_name = route_config.script.clone();
                if let Some(route_id) = api
                    .get_worker_route(zone_id.clone(), pattern, script_name)
                    .await?
                {
                    plan.remove_worker_routes.push((zone_id.clone(), route_id));
                }
            }
        }

        // 5) Cache ruleset
        let cache_name = &self.zone_resources.cache_rules.name;
        let cache_phase = &self.zone_resources.cache_rules.phase;
        for zone_id in api.zone_ids() {
            if let Some(ruleset_id) = api
                .get_ruleset(zone_id.clone(), cache_name.clone(), cache_phase.clone())
                .await?
            {
                plan.remove_rulesets.push((zone_id.clone(), ruleset_id));
            }
        }

        // 6) Account token
        let tokens = api.list_account_tokens().await?;
        let linkup_token = tokens
            .iter()
            .find(|token| token.name.as_deref() == Some(LINKUP_ACCOUNT_TOKEN_NAME));
        if let Some(token) = linkup_token {
            plan.remove_account_token = Some(token.id.to_string());
        }

        Ok(plan)
    }

    pub async fn execute_destroy_plan(
        &self,
        api: &impl CloudflareApi,
        plan: &DestroyPlan,
        notifier: &impl DeployNotifier,
    ) -> Result<(), DeployError> {
        // Remove routes and DNS records *first* to avoid orphan references:

        // 1) Worker routes
        for (zone_id, route_id) in &plan.remove_worker_routes {
            notifier.notify(&format!(
                "Removing worker route with ID '{}' in zone '{}'.",
                route_id, zone_id
            ));
            api.remove_worker_route(zone_id.clone(), route_id.clone())
                .await?;
            notifier.notify(&format!("Worker route '{}' removed.", route_id));
        }

        // 2) DNS records
        for (zone_id, record_id) in &plan.remove_dns_records {
            notifier.notify(&format!(
                "Removing DNS record with ID '{}' in zone '{}'.",
                record_id, zone_id
            ));
            api.remove_dns_record(zone_id.clone(), record_id.clone())
                .await?;
            notifier.notify(&format!("DNS record '{}' removed.", record_id));
        }

        // 3) Worker script
        if let Some(script_name) = &plan.remove_worker_script {
            notifier.notify(&format!("Removing worker script '{}'...", script_name));
            api.remove_worker_script(script_name.clone()).await?;
            notifier.notify("Worker script removed successfully.");
        }

        // 4) KV namespace
        if let Some(ns_id) = &plan.remove_kv_namespace {
            notifier.notify(&format!("Removing KV namespace '{}'...", self.kv_name));
            api.remove_kv_namespace(ns_id.clone()).await?;
            notifier.notify(&format!(
                "KV namespace '{}' removed successfully.",
                self.kv_name
            ));
        }

        // 5) Cache rulesets
        for (zone_id, ruleset_id) in &plan.remove_rulesets {
            notifier.notify(&format!(
                "Removing cache ruleset with ID '{}' in zone '{}'.",
                ruleset_id, zone_id
            ));
            api.remove_ruleset_rules(zone_id.clone(), ruleset_id.clone())
                .await?;
            notifier.notify(&format!("Cache ruleset '{}' removed.", ruleset_id));
        }

        // 6) Account token
        if let Some(token) = &plan.remove_account_token {
            notifier.notify("Removing Linkup account token...");
            api.remove_account_token(token).await?;
            notifier.notify("Linkup account token removed.");
        }

        Ok(())
    }

    pub fn worker_version_hash(&self) -> String {
        let mut hasher = Sha256::new();

        // Incorporate the "entry" file name so that changes to which file is
        // the main module also affect the hash.
        hasher.update(self.worker_script_entry.as_bytes());

        // For each part, incorporate the part's name, content type, and raw bytes
        // into the hash. The order is important, so if your system might reorder
        // `worker_script_parts`, consider sorting them by name first, or do something
        // else to keep the input stable.
        for part in &self.worker_script_parts {
            hasher.update(part.name.as_bytes());
            hasher.update(part.content_type.as_bytes());
            hasher.update(&part.data);
        }

        // Finalize the hasher and convert to a hex string
        let hash_bytes = hasher.finalize();
        hex::encode(hash_bytes)
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
            && self.account_token_action.is_none()
    }
}

/// Compare rules by their relevant fields only.
pub fn rules_equal(current: &[Rule], desired: &[Rule]) -> bool {
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
