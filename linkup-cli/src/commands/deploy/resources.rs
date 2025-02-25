use std::fmt;

use rand::Rng;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{api::CloudflareApi, cf_deploy::DeployNotifier, DeployError};

pub(super) const LINKUP_ACCOUNT_TOKEN_NAME: &str = "linkup-account-owned-cli-access-token";
// To build the worker script, run in the worker directory:
// cargo install -q worker-build && worker-build --release
const LINKUP_WORKER_SHIM: &[u8] = include_bytes!("../../../../worker/build/worker/shim.mjs");
const LINKUP_WORKER_INDEX_WASM: &[u8] =
    include_bytes!("../../../../worker/build/worker/index.wasm");

#[derive(Debug, Clone)]
pub struct TargetCfResources {
    pub account_id: String,
    pub worker_script_name: String,
    pub worker_script_parts: Vec<WorkerScriptPart>,
    pub worker_script_entry: String,
    pub worker_script_bindings: Vec<WorkerBinding>,
    pub worker_script_schedules: Vec<cloudflare::endpoints::workers::WorkersSchedule>,
    pub kv_namespaces: Vec<KvNamespace>,
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
    pub bindings: Vec<WorkerBinding>,
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
pub enum WorkerBinding {
    KvNamespace { name: String, namespace_id: String },
    PlainText { name: String, text: String },
    SecretText { name: String, text: String },
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
    pub kv_actions: Vec<KvPlan>,
    pub script_action: Option<WorkerScriptPlan>,
    pub worker_subdomain_action: Option<WorkerSubdomainPlan>,
    pub worker_schedules_action: Option<WorkerSchedulesPlan>,
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

#[derive(Debug, Clone)]
pub struct KvNamespace {
    pub name: String,
    pub binding: String,
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

#[derive(Debug)]
pub enum WorkerSubdomainPlan {
    /// Upload the script with the given metadata & parts
    Create { enabled: bool, script_name: String },
}

#[derive(Debug)]
pub struct WorkerSchedulesPlan {
    schedules: Vec<cloudflare::endpoints::workers::WorkersSchedule>,
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
    pub remove_kv_namespaces: Vec<String>,

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
            && self.remove_kv_namespaces.is_empty()
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
        cloudflare_client: &cloudflare::framework::async_api::Client,
    ) -> Result<DeployPlan, DeployError> {
        println!("Checking account token.");
        let account_token_action = self.check_account_token(api).await?;

        println!("Checking kv namespace.");
        let kv_action = self.check_kv_namespaces(api).await?;

        println!("Checking worker script.");
        let script_action = self
            .check_worker_script(api, cloudflare_client, &account_token_action)
            .await?;

        println!("Checking worker subdomain.");
        let worker_subdomain_action = self.check_worker_subdomain(api).await?;

        println!("Checking DNS records.");
        let dns_actions = self.check_dns_records(api).await?;

        println!("Checking worker routes.");
        let route_actions = self.check_worker_routes(api).await?;

        println!("Checking cache rulesets.");
        let ruleset_actions = self.check_rulesets(api).await?;

        println!("Checking worker schedules.");
        let worker_schedules_action = self.check_worker_schedules(cloudflare_client).await?;

        Ok(DeployPlan {
            kv_actions: kv_action,
            script_action,
            dns_actions,
            route_actions,
            ruleset_actions,
            account_token_action,
            worker_subdomain_action,
            worker_schedules_action,
        })
    }
    /// Check if the KV namespace exists and return the plan if not.
    pub async fn check_kv_namespaces(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Vec<KvPlan>, DeployError> {
        let mut plans = Vec::with_capacity(self.kv_namespaces.len());
        for kv_namespace in &self.kv_namespaces {
            let existing = api.get_kv_namespace_id(kv_namespace.name.clone()).await?;
            if existing.is_none() {
                // We need to create this KV namespace
                plans.push(KvPlan::Create {
                    namespace_name: kv_namespace.name.clone(),
                });
            }
        }

        Ok(plans)
    }

    /// Check if the worker script needs to be uploaded. Return the plan if an upload is needed.
    pub async fn check_worker_script(
        &self,
        api: &impl CloudflareApi,
        cloudflare_client: &cloudflare::framework::async_api::Client,
        account_token_plan: &Option<AccountTokenPlan>,
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
            let mut bindings = Vec::with_capacity(self.kv_namespaces.len());
            for kv_namespace in &self.kv_namespaces {
                bindings.push(WorkerBinding::KvNamespace {
                    name: kv_namespace.binding.clone(),
                    namespace_id: "<to-be-filled-on-deploy>".to_string(),
                });
            }

            for binding in &self.worker_script_bindings {
                bindings.push(binding.clone());
            }

            match self.check_worker_token(cloudflare_client).await? {
                Some(existing_token) => {
                    bindings.push(WorkerBinding::PlainText {
                        name: "WORKER_TOKEN".to_string(),
                        text: existing_token,
                    });
                }
                None => {
                    bindings.push(WorkerBinding::PlainText {
                        name: "WORKER_TOKEN".to_string(),
                        text: generate_secret(),
                    });
                }
            }

            if account_token_plan.is_some() {
                bindings.push(WorkerBinding::SecretText {
                    name: "CLOUDFLARE_API_TOKEN".to_string(),
                    text: "<to-be-filled-on-deploy>".to_string(),
                });
            }

            // Construct the metadata
            let metadata = WorkerMetadata {
                main_module: self.worker_script_entry.clone(),
                bindings,
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

    pub async fn check_worker_subdomain(
        &self,
        api: &impl CloudflareApi,
    ) -> Result<Option<WorkerSubdomainPlan>, DeployError> {
        let script_name = &self.worker_script_name;
        if let Ok(subdomain) = api.get_worker_subdomain(script_name.clone()).await {
            if !subdomain.enabled {
                return Ok(Some(WorkerSubdomainPlan::Create {
                    enabled: true,
                    script_name: script_name.clone(),
                }));
            } else {
                return Ok(None);
            }
        }
        Ok(Some(WorkerSubdomainPlan::Create {
            enabled: true,
            script_name: script_name.clone(),
        }))
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
            let zone_name = api.get_zone_name(zone_id).await?;
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

    /// Check if the worker already has a binding of a worker token. We don't want to create a new one
    /// on every deploy, so we use this to check if one already exists.
    pub async fn check_worker_token(
        &self,
        client: &cloudflare::framework::async_api::Client,
    ) -> Result<Option<String>, DeployError> {
        let req = cloudflare::endpoints::workers::ListBindings {
            account_id: &self.account_id,
            script_name: &self.worker_script_name,
        };

        let bindings = match client.request(&req).await {
            Ok(response) => response.result,
            Err(cloudflare::framework::response::ApiFailure::Error(StatusCode::NOT_FOUND, _)) => {
                return Ok(None)
            }
            Err(error) => return Err(DeployError::from(error)),
        };

        for binding in bindings {
            if binding.name == "WORKER_TOKEN" {
                if let Some(text) = binding.text {
                    return Ok(Some(text));
                }
            }
        }

        Ok(None)
    }

    pub async fn check_worker_schedules(
        &self,
        client: &cloudflare::framework::async_api::Client,
    ) -> Result<Option<WorkerSchedulesPlan>, DeployError> {
        let req = cloudflare::endpoints::workers::ListSchedules {
            account_identifier: &self.account_id,
            script_name: &self.worker_script_name,
        };

        let mut existing_schedules = match client.request(&req).await {
            Ok(response) => response.result.schedules,
            Err(cloudflare::framework::response::ApiFailure::Error(StatusCode::NOT_FOUND, _)) => {
                return Ok(None)
            }
            Err(error) => return Err(DeployError::from(error)),
        };

        if existing_schedules.len() != self.worker_script_schedules.len() {
            return Ok(Some(WorkerSchedulesPlan {
                schedules: self.worker_script_schedules.clone(),
            }));
        }

        existing_schedules.sort();

        let mut planned_schedules = self.worker_script_schedules.clone();
        planned_schedules.sort();

        let matching = existing_schedules
            .iter()
            .zip(planned_schedules.iter())
            .filter(|&(a, b)| a.cron == b.cron)
            .count();

        if matching != existing_schedules.len() {
            return Ok(Some(WorkerSchedulesPlan {
                schedules: self.worker_script_schedules.clone(),
            }));
        }

        Ok(None)
    }

    pub async fn execute_deploy_plan(
        &self,
        api: &impl CloudflareApi,
        client: &cloudflare::framework::async_api::Client,
        plan: &DeployPlan,
        notifier: &impl DeployNotifier,
    ) -> Result<(), DeployError> {
        // We may need the worker subdomain for DNS records, so fetch it once:
        let worker_subdomain = api.get_account_worker_subdomain().await?;

        for kv_plan in &plan.kv_actions {
            match kv_plan {
                KvPlan::Create { namespace_name } => {
                    notifier.notify(&format!("Creating KV namespace: {}", namespace_name));
                    let new_id = api.create_kv_namespace(namespace_name.clone()).await?;
                    notifier.notify(&format!("KV namespace created with ID: {}", new_id));
                }
            }
        }

        let mut token: Option<String> = None;
        if let Some(AccountTokenPlan { token_name }) = &plan.account_token_action {
            notifier.notify("Creating account token...");

            let created_token = api.create_account_token(token_name).await?;
            token = Some(created_token.clone());
        }

        if let Some(WorkerScriptPlan::Upload {
            script_name,
            metadata,
            parts,
        }) = &plan.script_action
        {
            let mut final_metadata = metadata.clone();

            for kv_namespace in &self.kv_namespaces {
                let kv_ns_id = api.get_kv_namespace_id(kv_namespace.name.clone()).await?;
                let kv_ns_id = kv_ns_id.ok_or_else(|| {
                    DeployError::UnexpectedResponse(
                        "KV namespace should exist but was not found".to_string(),
                    )
                })?;

                for binding in final_metadata.bindings.iter_mut() {
                    if let WorkerBinding::KvNamespace { name, namespace_id } = binding {
                        if *name == kv_namespace.binding {
                            *namespace_id = kv_ns_id.clone();
                            break;
                        }
                    }
                }
            }

            if let Some(token) = token {
                for binding in final_metadata.bindings.iter_mut() {
                    if let WorkerBinding::SecretText { name, text } = binding {
                        if *name == "CLOUDFLARE_API_TOKEN" {
                            *text = token.clone();
                        }
                    }
                }
            }

            notifier.notify("Uploading worker script...");
            api.create_worker_script(script_name.clone(), final_metadata, parts.clone())
                .await?;
            notifier.notify("Worker script uploaded successfully.");
        }

        if let Some(WorkerSubdomainPlan::Create {
            enabled,
            script_name,
        }) = &plan.worker_subdomain_action
        {
            notifier.notify("Updating worker subdomain...");
            api.post_worker_subdomain(script_name.clone(), *enabled, None)
                .await?;
            notifier.notify("Worker subdomain updated successfully.");
        }

        for dns_plan in &plan.dns_actions {
            let DnsRecordPlan::Create { zone_id, record } = dns_plan;
            let final_record = {
                let mut r = record.clone();
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

        for ruleset_plan in &plan.ruleset_actions {
            let RulesetPlan {
                zone_id,
                ruleset_id,
                create_new,
                rules,
            } = ruleset_plan;

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

        if let Some(WorkerSchedulesPlan { schedules }) = &plan.worker_schedules_action {
            notifier.notify("Upserting worker schedules...");

            let schedules = schedules.clone();

            let req = cloudflare::endpoints::workers::UpsertSchedules {
                account_identifier: &self.account_id,
                script_name: &self.worker_script_name,
                schedules,
            };

            client.request(&req).await?;
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
        for kv_namespace in &self.kv_namespaces {
            if let Some(ns_id) = api.get_kv_namespace_id(kv_namespace.name.clone()).await? {
                plan.remove_kv_namespaces.push(ns_id);
            }
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
            let zone_name = api.get_zone_name(zone_id).await?;
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
        for ns_id in &plan.remove_kv_namespaces {
            notifier.notify(&format!("Removing KV namespace '{}'...", ns_id));
            api.remove_kv_namespace(ns_id.clone()).await?;
            notifier.notify(&format!("KV namespace '{}' removed successfully.", ns_id));
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
        self.kv_actions.is_empty()
            && self.script_action.is_none()
            && self.dns_actions.is_empty()
            && self.route_actions.is_empty()
            && self.ruleset_actions.is_empty()
            && self.account_token_action.is_none()
            && self.worker_schedules_action.is_none()
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

pub fn cf_resources(
    account_id: String,
    tunnel_zone_id: String,
    all_zone_names: &[String],
    all_zone_ids: &[String],
) -> TargetCfResources {
    let joined_zone_names = all_zone_names.join("-").replace(".", "-");
    let linkup_script_name = format!("linkup-worker-{joined_zone_names}");

    TargetCfResources {
        account_id: account_id.clone(),
        worker_script_name: linkup_script_name.clone(),
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
        worker_script_bindings: vec![
            WorkerBinding::PlainText {
                name: "CLOUDFLARE_ACCOUNT_ID".to_string(),
                text: account_id,
            },
            WorkerBinding::PlainText {
                name: "CLOUDFLARE_TUNNEL_ZONE_ID".to_string(),
                text: tunnel_zone_id,
            },
            WorkerBinding::PlainText {
                name: "CLOUDLFLARE_ALL_ZONE_IDS".to_string(),
                text: all_zone_ids.join(","),
            },
        ],
        worker_script_schedules: vec![cloudflare::endpoints::workers::WorkersSchedule {
            cron: Some("0 12 * * 2-6".to_string()),
            ..Default::default()
        }],
        kv_namespaces: vec![
            KvNamespace {
                name: format!("linkup-session-kv-{joined_zone_names}"),
                binding: "LINKUP_SESSIONS".to_string(),
            },
            KvNamespace {
                name: format!("linkup-tunnels-kv-{joined_zone_names}"),
                binding: "LINKUP_TUNNELS".to_string(),
            },
            KvNamespace {
                name: format!("linkup-certificate-cache-kv-{joined_zone_names}"),
                binding: "LINKUP_CERTIFICATE_CACHE".to_string(),
            },
        ],
        zone_resources: TargectCfZoneResources {
            dns_records: vec![
                TargetDNSRecord {
                    route: "*".to_string(),
                    script: linkup_script_name.clone(),
                },
                TargetDNSRecord {
                    route: "@".to_string(),
                    script: linkup_script_name.clone(),
                },
            ],
            routes: vec![
                TargetWorkerRoute {
                    route: "*.".to_string(),
                    script: linkup_script_name.clone(),
                },
                TargetWorkerRoute {
                    route: "".to_string(),
                    script: linkup_script_name.clone(),
                },
            ],
            cache_rules: TargetCacheRules {
                name: "default".to_string(),
                phase: "http_request_cache_settings".to_string(),
                rules: vec![Rule {
                    action: "set_cache_settings".to_string(),
                    description: "linkup cache rule - do not cache tunnel requests".to_string(),
                    enabled: true,
                    expression: "(starts_with(http.host, \"linkup-tunnel-\"))".to_string(),
                    action_parameters: Some(serde_json::json!({"cache": false})),
                }],
            },
        },
    }
}

pub fn generate_secret() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();

    base64::Engine::encode(&base64::prelude::BASE64_STANDARD, bytes)
}
