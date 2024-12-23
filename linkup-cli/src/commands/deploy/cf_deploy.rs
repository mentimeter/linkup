use std::env;

use super::cf_api::{AccountCloudflareApi, CloudflareApi, Rule};
use super::cf_auth::CloudflareTokenAuth;
use super::console_notify::ConsoleNotifier;

#[derive(thiserror::Error, Debug)]
pub enum DeployError {
    #[error("Cloudflare API error: {0}")]
    CloudflareApiError(#[from] reqwest::Error),
    #[error("Unexpected Cloudflare API response: {0}")]
    UnexpectedResponse(String),
    #[error("Other failure")]
    OtherError,
}

#[derive(Debug, Clone)]
pub struct TargetCfResources {
    worker_script_name: String,
    worker_script_parts: Vec<WorkerScriptPart>,
    kv_name: String,
    zone_resources: TargectCfZoneResources,
}

#[derive(Debug, Clone)]
pub struct TargectCfZoneResources {
    routes: Vec<TargectCfRoutes>,
    cache_rules: TargetCacheRules,
}

#[derive(Debug, Clone)]
pub struct TargetCacheRules {
    name: String,
    phase: String,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct TargectCfRoutes {
    route: String,
    script: String,
}

impl TargectCfRoutes {
    pub fn comment(&self) -> String {
        format!("{}-{}", self.script, self.route)
    }

    pub fn worker_route(&self, zone_name: String) -> String {
        format!("{}.{}/*", self.route, zone_name)
    }
}

#[derive(Debug, Clone)]
pub struct WorkerScriptInfo {
    pub id: String,
}

pub struct WorkerMetadata {
    pub main_module: String,
    pub bindings: Vec<WorkerBinding>,
    pub compatibility_date: String,
}

#[derive(Debug, Clone)]
pub struct WorkerScriptPart {
    pub name: String,
    pub data: Vec<u8>,
}

pub struct WorkerBinding {
    pub type_: String,
    pub name: String,
    pub text: String,
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

pub trait DeployNotifier {
    fn ask_confirmation(&self) -> bool;
    fn notify(&self, message: &str);
}

const LOCAL_SCRIPT_CONTENT: &str = r#"
export default {
	async fetch(request, env, ctx) {
		return new Response('Hello World!');
	},
};
"#;

#[derive(clap::Args)]
pub struct Args {
    #[arg(
        short = 'a',
        long = "account-id",
        help = "Cloudflare account ID",
        value_name = "ACCOUNT_ID"
    )]
    account_id: String,

    #[arg(
        short = 'z',
        long = "zone-ids",
        help = "Cloudflare zone IDs",
        value_name = "ZONE_IDS",
        num_args = 1..,
        required = true
    )]
    zone_ids: Vec<String>,
}

pub async fn deploy(args: &Args) -> Result<(), DeployError> {
    println!("Deploying to Cloudflare...");
    println!("Account ID: {}", args.account_id);
    println!("Zone IDs: {:?}", args.zone_ids);

    let api_key = env::var("CLOUDFLARE_API_KEY").expect("Missing Cloudflare API token");
    let zone_ids_strings: Vec<String> = args.zone_ids.iter().map(|s| s.to_string()).collect();

    let token_auth = CloudflareTokenAuth::new(api_key);

    let cloudflare_api = AccountCloudflareApi::new(
        args.account_id.to_string(),
        zone_ids_strings,
        Box::new(token_auth),
    );
    let notifier = ConsoleNotifier::new();

    let resources = TargetCfResources {
        worker_script_name: "linkup-integration-test-script".to_string(),
        worker_script_parts: vec![WorkerScriptPart {
            name: "index.js".to_string(),
            data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
        }],
        kv_name: "linkup-integration-test-kv".to_string(),
        zone_resources: TargectCfZoneResources {
            routes: vec![TargectCfRoutes {
                route: "linkup-integraton-test".to_string(),
                script: "linkup-integration-test-script".to_string(),
            }],
            cache_rules: TargetCacheRules {
                name: "linkup-integration-test-cache-rules".to_string(),
                phase: "http_request_firewall_custom".to_string(),
                rules: vec![Rule {
                    action: "block".to_string(),
                    description: "test linkup integration rule".to_string(),
                    enabled: true,
                    expression: "(starts_with(http.host, \"does-not-exist-host\"))".to_string(),
                    // action_parameters: Some(serde_json::json!({"cache": false})),
                    action_parameters: None,
                }],
            },
        },
    };

    deploy_to_cloudflare(&resources, &cloudflare_api, &notifier).await?;

    Ok(())
}

async fn deploy_to_cloudflare(
    resources: &TargetCfResources,
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    let script_name = &resources.worker_script_name;

    let existing_info = api.get_worker_script_info(script_name.clone()).await?;
    let needs_upload = if let Some(_) = existing_info {
        let existing_content = api.get_worker_script_content(script_name.clone()).await?;
        existing_content != LOCAL_SCRIPT_CONTENT
    } else {
        true
    };

    if needs_upload {
        notifier.notify("Worker script differs or does not exist.");
        let confirmed = notifier.ask_confirmation();
        if confirmed {
            let metadata = WorkerMetadata {
                main_module: "index.js".to_string(),
                bindings: vec![],
                compatibility_date: "2024-12-18".to_string(),
            };

            api.create_worker_script(
                script_name.to_string(),
                metadata,
                resources.worker_script_parts.clone(),
            )
            .await?;
            notifier.notify("Worker script uploaded successfully.");
        } else {
            notifier.notify("Deployment canceled by user.");
            return Ok(());
        }
    } else {
        notifier.notify("No changes needed. Worker script is already up to date.");
    }

    // Handle KV namespace
    let kv_name = &resources.kv_name;
    let kv_ns_id = api.get_kv_namespace_id(kv_name.clone()).await?;
    if kv_ns_id.is_none() {
        notifier.notify(&format!(
            "KV namespace '{}' does not exist. Creating...",
            kv_name
        ));
        let new_id = api.create_kv_namespace(kv_name.clone()).await?;
        notifier.notify(&format!(
            "KV namespace '{}' created with ID: {}",
            kv_name, new_id
        ));
    } else {
        notifier.notify(&format!("KV namespace '{}' already exists.", kv_name));
    }

    // Determine subdomain for script
    let subdomain = api.get_worker_subdomain().await?;
    let cname_target = if let Some(sub) = subdomain {
        format!("{}.{}.workers.dev", script_name, sub)
    } else {
        format!("{}.workers.dev", script_name)
    };

    // For each zone, ensure DNS record and Worker route
    // Assuming route = DNS name (e.g. "linkup-integraton-test" means "linkup-integraton-test.example.com")
    // Adjust logic as needed.
    for zone_id in api.zone_ids() {
        for route_config in &resources.zone_resources.routes {
            let route = &route_config.route;
            let script = &route_config.script;
            // DNS Record Check
            let existing_dns = api
                .get_dns_record(zone_id.clone(), route_config.comment())
                .await?;
            if existing_dns.is_none() {
                notifier.notify(&format!(
                    "DNS record for '{}' not found in zone '{}'. Creating...",
                    route, zone_id
                ));
                let new_record = DNSRecord {
                    id: "".to_string(),
                    name: route.clone(),
                    record_type: "CNAME".to_string(),
                    content: cname_target.clone(),
                    comment: route_config.comment(),
                    proxied: true,
                };
                api.create_dns_record(zone_id.clone(), new_record).await?;
                notifier.notify(&format!(
                    "DNS record for '{}' created pointing to '{}'",
                    route, cname_target
                ));
            } else {
                notifier.notify(&format!(
                    "DNS record for '{}' already exists in zone '{}'.",
                    route, zone_id
                ));
            }

            let zone_name = api.get_zone_name(zone_id.clone()).await?;
            // Worker Route Check
            let existing_route = api
                .get_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name.clone()),
                    script.clone(),
                )
                .await?;

            if existing_route.is_none() {
                notifier.notify(&format!(
                    "Worker route for pattern '{}' and script '{}' not found in zone '{}'. Creating...",
                    route_config.route, route_config.script, zone_id
                ));

                api.create_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name),
                    script.clone(),
                )
                .await?;
                notifier.notify(&format!(
                    "Worker route for pattern '{}' and script '{}' created",
                    route_config.route, route_config.script
                ));
            } else {
                notifier.notify(&format!(
                    "Worker route for pattern '{}' and script '{}' already exists in zone '{}'.",
                    route_config.route, route_config.script, zone_id
                ));
            }
        }
    }

    for zone_id in api.zone_ids() {
        let cache_name = resources.zone_resources.cache_rules.name.clone();
        let cache_phase = resources.zone_resources.cache_rules.phase.clone();
        let cache_ruleset = api
            .get_ruleset(zone_id.clone(), cache_name.clone(), cache_phase.clone())
            .await?;
        println!("got ruleset {:?}", cache_ruleset);
        let ruleset_id = if cache_ruleset.is_none() {
            notifier.notify("Cache ruleset does not exist. Creating...");
            println!("creating ruleset");
            let new_ruleset_id = api
                .create_ruleset(zone_id.clone(), cache_name.clone(), cache_phase.clone())
                .await?;
            println!("created ruleset");
            notifier.notify(&format!(
                "Cache ruleset created with ID: {}",
                new_ruleset_id
            ));
            new_ruleset_id
        } else {
            cache_ruleset.unwrap()
        };

        println!("got ruleset id {:?}", ruleset_id);
        // 2. Get current rules
        let current_rules = api
            .get_ruleset_rules(zone_id.clone(), ruleset_id.clone())
            .await?;
        let desired_rules = &resources.zone_resources.cache_rules.rules;
        println!("got rules {:?}", current_rules);

        // Compare current and desired rules
        if !rules_equal(&current_rules, &desired_rules) {
            notifier.notify("Cache rules differ. Updating ruleset...");
            api.update_ruleset_rules(zone_id.clone(), ruleset_id.clone(), desired_rules.clone())
                .await?;
            notifier.notify("Cache ruleset updated successfully.");
        } else {
            notifier.notify("Cache ruleset matches desired configuration. No changes needed.");
        }
    }

    Ok(())
}

// Helper function to compare sets of rules ignoring fields like `id`, `last_updated`, `ref`, `version` that might not be in `Rule` struct
fn rules_equal(current: &Vec<Rule>, desired: &Vec<Rule>) -> bool {
    if current.len() != desired.len() {
        return false;
    }

    // We'll compare rules by action, description, enabled, expression, and action_parameters.
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

pub async fn destroy_from_cloudflare(
    resources: &TargetCfResources,
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    let script_name = &resources.worker_script_name;
    let kv_name = &resources.kv_name;

    // For each zone, remove routes and DNS records
    for zone_id in api.zone_ids() {
        for route_config in &resources.zone_resources.routes {
            let route = &route_config.route;
            let script = &route_config.script;

            let zone_name = api.get_zone_name(zone_id.clone()).await?;

            // Remove Worker route if it exists
            let existing_route = api
                .get_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name.clone()),
                    script.clone(),
                )
                .await?;
            if let Some(route_id) = existing_route {
                notifier.notify(&format!(
                    "Removing worker route for pattern '{}' and script '{}' in zone '{}'.",
                    route, script, zone_id
                ));
                api.remove_worker_route(zone_id.clone(), route_id).await?;
                notifier.notify(&format!(
                    "Worker route for pattern '{}' and script '{}' removed.",
                    route, script
                ));
            } else {
                notifier.notify(&format!(
                    "No worker route for pattern '{}' and script '{}' found in zone '{}', nothing to remove.",
                    route, script, zone_id
                ));
            }

            // Remove DNS record if it exists
            let existing_dns = api
                .get_dns_record(zone_id.clone(), route_config.comment())
                .await?;
            if let Some(record) = existing_dns {
                notifier.notify(&format!(
                    "Removing DNS record '{}' in zone '{}'.",
                    record.name, zone_id
                ));
                api.remove_dns_record(zone_id.clone(), record.id.clone())
                    .await?;
                notifier.notify(&format!("DNS record '{}' removed.", record.name));
            } else {
                notifier.notify(&format!(
                    "No DNS record for '{}' found in zone '{}', nothing to remove.",
                    route, zone_id
                ));
            }
        }
    }

    // Remove the KV namespace if it exists
    let kv_ns_id = api.get_kv_namespace_id(kv_name.clone()).await?;
    if let Some(ns_id) = kv_ns_id {
        notifier.notify(&format!("Removing KV namespace '{}'...", kv_name));
        api.remove_kv_namespace(ns_id.clone()).await?;
        notifier.notify(&format!("KV namespace '{}' removed successfully.", kv_name));
    } else {
        notifier.notify(&format!(
            "KV namespace '{}' does not exist, nothing to remove.",
            kv_name
        ));
    }

    // Remove the Worker script
    let existing_info = api.get_worker_script_info(script_name.clone()).await?;
    if existing_info.is_some() {
        notifier.notify(&format!("Removing worker script '{}'...", script_name));
        api.remove_worker_script(script_name.to_string()).await?;
        notifier.notify("Worker script removed successfully.");
    } else {
        notifier.notify(&format!(
            "Worker script '{}' does not exist, nothing to remove.",
            script_name
        ));
    }

    for zone_id in api.zone_ids() {
        let cache_name = resources.zone_resources.cache_rules.name.clone();
        let cache_phase = resources.zone_resources.cache_rules.phase.clone();
        let cache_ruleset = api
            .get_ruleset(zone_id.clone(), cache_name.clone(), cache_phase.clone())
            .await?;
        if let Some(ruleset_id) = cache_ruleset {
            notifier.notify(&format!(
                "Removing cache ruleset '{}' in zone '{}'.",
                cache_name, zone_id
            ));
            api.remove_ruleset_rules(zone_id.clone(), ruleset_id.clone())
                .await?;
            notifier.notify(&format!("Cache ruleset '{}' removed.", cache_name));
        } else {
            notifier.notify(&format!(
                "Cache ruleset '{}' does not exist in zone '{}', nothing to remove.",
                cache_name, zone_id
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crate::commands::deploy;

    use super::*;

    struct TestCloudflareApi {
        zone_ids: Vec<String>,

        pub existing_info: Option<WorkerScriptInfo>,
        pub existing_content: Option<String>,
        pub create_called_with: RefCell<Option<(String, WorkerMetadata, Vec<WorkerScriptPart>)>>,

        pub dns_records: RefCell<Vec<DNSRecord>>,
        pub worker_routes: RefCell<Vec<(String, String, String)>>,
    }

    impl TestCloudflareApi {
        fn new(zone_ids: Vec<String>) -> Self {
            Self {
                zone_ids,
                existing_info: None,
                existing_content: None,
                create_called_with: RefCell::new(None),
                dns_records: RefCell::new(vec![]),
                worker_routes: RefCell::new(vec![]),
            }
        }
    }

    impl CloudflareApi for TestCloudflareApi {
        fn zone_ids(&self) -> &Vec<String> {
            &self.zone_ids
        }

        async fn get_worker_script_content(
            &self,
            script_name: String,
        ) -> Result<String, DeployError> {
            if let Some(content) = &self.existing_content {
                Ok(content.clone())
            } else {
                Err(DeployError::OtherError)
            }
        }

        async fn get_worker_script_info(
            &self,
            _script_name: String,
        ) -> Result<Option<WorkerScriptInfo>, DeployError> {
            Ok(self.existing_info.clone())
        }

        async fn create_worker_script(
            &self,
            script_name: String,
            metadata: WorkerMetadata,
            parts: Vec<WorkerScriptPart>,
        ) -> Result<(), DeployError> {
            *self.create_called_with.borrow_mut() = Some((script_name, metadata, parts));
            Ok(())
        }

        async fn remove_worker_script(&self, script_name: String) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_kv_namespace_id(
            &self,
            namespace_name: String,
        ) -> Result<Option<String>, DeployError> {
            Ok(None)
        }

        async fn create_kv_namespace(&self, namespace_id: String) -> Result<String, DeployError> {
            Ok("new-namespace-id".to_string())
        }

        async fn remove_kv_namespace(&self, namespace_id: String) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_dns_record(
            &self,
            _zone_id: String,
            record_tag: String,
        ) -> Result<Option<DNSRecord>, DeployError> {
            let records = self.dns_records.borrow();
            for r in records.iter() {
                if r.name == record_tag {
                    return Ok(Some((*r).clone()));
                }
            }
            Ok(None)
        }

        async fn create_dns_record(
            &self,
            _zone_id: String,
            record: DNSRecord,
        ) -> Result<(), DeployError> {
            self.dns_records.borrow_mut().push(record);
            Ok(())
        }

        async fn remove_dns_record(
            &self,
            _zone_id: String,
            record_id: String,
        ) -> Result<(), DeployError> {
            let mut records = self.dns_records.borrow_mut();
            records.retain(|r| r.id != record_id);
            Ok(())
        }

        async fn get_worker_subdomain(&self) -> Result<Option<String>, DeployError> {
            // Simulate having no subdomain for testing:
            Ok(None)
        }

        async fn get_worker_route(
            &self,
            zone_id: String,
            pattern: String,
            script_name: String,
        ) -> Result<Option<String>, DeployError> {
            let routes = self.worker_routes.borrow();
            for (z, p, s) in routes.iter() {
                if *z == zone_id && *p == pattern && *s == script_name {
                    return Ok(Some(format!("route-id-for-{}", p)));
                }
            }
            Ok(None)
        }

        async fn create_worker_route(
            &self,
            zone_id: String,
            pattern: String,
            script_name: String,
        ) -> Result<(), DeployError> {
            self.worker_routes
                .borrow_mut()
                .push((zone_id, pattern, script_name));
            Ok(())
        }

        async fn remove_worker_route(
            &self,
            zone_id: String,
            route_id: String,
        ) -> Result<(), DeployError> {
            let mut routes = self.worker_routes.borrow_mut();
            routes
                .retain(|(z, p, s)| !(z == &zone_id && format!("route-id-for-{}", p) == route_id));
            Ok(())
        }

        async fn get_zone_name(&self, zone_id: String) -> Result<String, DeployError> {
            Ok("example.com".to_string())
        }

        async fn get_ruleset(
            &self,
            zone_id: String,
            name: String,
            phase: String,
        ) -> Result<Option<String>, DeployError> {
            Ok(None)
        }

        async fn create_ruleset(
            &self,
            zone_id: String,
            name: String,
            phase: String,
        ) -> Result<String, DeployError> {
            Ok("new-ruleset-id".to_string())
        }

        async fn update_ruleset_rules(
            &self,
            zone_id: String,
            ruleset: String,
            rules: Vec<deploy::cf_api::Rule>,
        ) -> Result<(), DeployError> {
            Ok(())
        }

        async fn remove_ruleset_rules(
            &self,
            zone_id: String,
            ruleset: String,
        ) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_ruleset_rules(
            &self,
            zone_id: String,
            ruleset: String,
        ) -> Result<Vec<deploy::cf_api::Rule>, DeployError> {
            Ok(vec![])
        }
    }

    struct TestNotifier {
        pub messages: RefCell<Vec<String>>,
        pub confirmation_response: bool,
        pub confirmations_asked: RefCell<usize>,
    }

    impl DeployNotifier for TestNotifier {
        fn ask_confirmation(&self) -> bool {
            *self.confirmations_asked.borrow_mut() += 1;
            self.confirmation_response
        }

        fn notify(&self, message: &str) {
            self.messages.borrow_mut().push(message.to_string());
        }
    }

    fn test_resources() -> TargetCfResources {
        TargetCfResources {
            worker_script_name: "linkup-integration-test-script".to_string(),
            worker_script_parts: vec![WorkerScriptPart {
                name: "index.js".to_string(),
                data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
            }],
            kv_name: "linkup-integration-test-kv".to_string(),
            zone_resources: TargectCfZoneResources {
                routes: vec![TargectCfRoutes {
                    route: "linkup-integraton-test".to_string(),
                    script: "linkup-integration-test-script".to_string(),
                }],
                cache_rules: TargetCacheRules {
                    name: "linkup-integration-test-cache-rules".to_string(),
                    phase: "http_request_firewall_custom".to_string(),
                    rules: vec![Rule {
                        action: "block".to_string(),
                        description: "test linkup integration rule".to_string(),
                        enabled: true,
                        expression: "(starts_with(http.host, \"does-not-exist-host\"))".to_string(),
                        // action_parameters: Some(serde_json::json!({"cache": false})),
                        action_parameters: None,
                    }],
                },
            },
        }
    }

    #[tokio::test]
    async fn test_deploy_to_cloudflare_creates_script_when_none_exists() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        // Check that a worker script was created
        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "linkup-integration-test-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 0);
        assert_eq!(metadata.compatibility_date, "2024-12-18");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "index.js");
        assert_eq!(
            String::from_utf8(parts[0].data.clone()).unwrap(),
            LOCAL_SCRIPT_CONTENT
        );
    }

    #[tokio::test]
    async fn test_deploy_to_cloudflare_creates_script_when_content_differs() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "linkup-integration-test-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 0);
        assert_eq!(metadata.compatibility_date, "2024-12-18");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "index.js");
        assert_eq!(
            String::from_utf8(parts[0].data.clone()).unwrap(),
            LOCAL_SCRIPT_CONTENT
        );
    }

    #[tokio::test]
    async fn test_deploy_to_cloudflare_canceled_by_user() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: false,
            confirmations_asked: RefCell::new(0),
        };

        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        assert!(created.is_none());
    }

    #[tokio::test]
    async fn test_deploy_creates_dns_and_route_if_not_exist() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let res = test_resources();

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&res, &api, &notifier).await;
        assert!(result.is_ok());

        // Check DNS record created
        let dns_records = api.dns_records.borrow();
        assert_eq!(dns_records.len(), 1);
        assert_eq!(dns_records[0].name, "linkup-integraton-test");
        assert!(dns_records[0]
            .content
            .contains("linkup-integration-test-script.workers.dev"));

        // Check route created
        let routes = api.worker_routes.borrow();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].1, "linkup-integraton-test");
        assert_eq!(routes[0].2, "linkup-integration-test-script");
    }

    #[tokio::test]
    async fn test_deploy_and_destroy_real_integration() {
        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        // Skip test if environment variables aren't set
        let account_id = match std::env::var("CLOUDFLARE_ACCOUNT_ID") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Skipping test: CLOUDFLARE_ACCOUNT_ID is not set");
                return;
            }
        };

        let zone_id = match std::env::var("CLOUDFLARE_ZONE_ID") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Skipping test: CLOUDFLARE_ZONE_ID is not set");
                return;
            }
        };

        let run_integration_test = match std::env::var("LINKUP_DEPLOY_INTEGRATION_TEST") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Skipping test: LINKUP_DEPLOY_INTEGRATION_TEST is not set");
                return;
            }
        };

        if run_integration_test != "true" {
            eprintln!("Skipping test: LINKUP_DEPLOY_INTEGRATION_TEST is not set to 'true'");
            return;
        }

        // Set up Cloudflare API
        let api_key = std::env::var("CLOUDFLARE_API_KEY").expect("CLOUDFLARE_API_KEY is not set");
        let email = std::env::var("CLOUDFLARE_EMAIL").expect("CLOUDFLARE_EMAIL is not set");

        let global_api_auth =
            deploy::cf_auth::CloudflareGlobalTokenAuth::new(api_key.clone(), email);
        let cloudflare_api = AccountCloudflareApi::new(
            account_id.clone(),
            vec![zone_id.to_string()],
            Box::new(global_api_auth),
        );

        // Deploy the resources
        let res = test_resources();
        let result = deploy_to_cloudflare(&res, &cloudflare_api, &notifier).await;
        assert!(result.is_ok(), "Deploy failed: {:?}", result);

        // Verify the worker
        let script_name = &res.worker_script_name;
        let worker_info = cloudflare_api
            .get_worker_script_info(script_name.clone())
            .await;
        assert!(
            worker_info.is_ok(),
            "Failed to get worker info: {:?}",
            worker_info
        );
        assert!(
            worker_info.unwrap().is_some(),
            "Worker script not found after deploy."
        );

        // Verify the KV namespace
        let kv_name = &res.kv_name;
        let kv_ns_id = cloudflare_api.get_kv_namespace_id(kv_name.clone()).await;
        assert!(
            kv_ns_id.is_ok(),
            "Failed to get KV namespace info: {:?}",
            kv_ns_id
        );
        assert!(
            kv_ns_id.unwrap().is_some(),
            "KV namespace not found after deploy."
        );

        // Verify DNS record exists
        for route_config in &res.zone_resources.routes {
            let existing_dns = cloudflare_api
                .get_dns_record(zone_id.clone(), route_config.comment())
                .await;
            assert!(
                existing_dns.is_ok(),
                "Failed to get DNS record for '{}': {:?}",
                route_config.route,
                existing_dns
            );
            assert!(
                existing_dns.unwrap().is_some(),
                "DNS record for '{}' not found after deploy.",
                route_config.route
            );
        }

        // Verify Worker route exists
        let zone_name = cloudflare_api.get_zone_name(zone_id.clone()).await.unwrap();
        for route_config in &res.zone_resources.routes {
            let existing_route = cloudflare_api
                .get_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name.clone()),
                    route_config.script.clone(),
                )
                .await;
            assert!(
                existing_route.is_ok(),
                "Failed to get worker route for pattern '{}': {:?}",
                route_config.route,
                existing_route
            );
            assert!(
                existing_route.unwrap().is_some(),
                "Worker route for pattern '{}' not found after deploy.",
                route_config.route
            );
        }

        // Verify cache ruleset exists
        let ruleset_id = cloudflare_api
            .get_ruleset(
                zone_id.clone(),
                res.zone_resources.cache_rules.name.clone(),
                res.zone_resources.cache_rules.phase.clone(),
            )
            .await
            .unwrap();
        assert!(
            ruleset_id.is_some(),
            "Cache ruleset should exist after deploy"
        );
        let ruleset_id = ruleset_id.unwrap();
        let current_rules = cloudflare_api
            .get_ruleset_rules(zone_id.clone(), ruleset_id.clone())
            .await
            .unwrap();
        assert!(
            rules_equal(&current_rules, &res.zone_resources.cache_rules.rules),
            "Cache ruleset should match desired rules after deploy"
        );

        // Destroy resources
        let destroy_result = destroy_from_cloudflare(&res, &cloudflare_api, &notifier).await;
        assert!(
            destroy_result.is_ok(),
            "Destroy failed: {:?}",
            destroy_result
        );

        // Verify worker is gone
        let worker_info = cloudflare_api
            .get_worker_script_info(script_name.clone())
            .await;
        assert!(
            worker_info.is_ok(),
            "Failed to get worker info after destroy: {:?}",
            worker_info
        );
        assert!(
            worker_info.unwrap().is_none(),
            "Worker script still exists after destroy"
        );

        // Verify KV namespace is gone
        let kv_ns_id = cloudflare_api.get_kv_namespace_id(kv_name.clone()).await;
        assert!(
            kv_ns_id.is_ok(),
            "Failed to get KV namespace after destroy: {:?}",
            kv_ns_id
        );
        assert!(
            kv_ns_id.unwrap().is_none(),
            "KV namespace still exists after destroy"
        );

        // Verify DNS record is gone
        for route_config in &res.zone_resources.routes {
            let existing_dns = cloudflare_api
                .get_dns_record(zone_id.clone(), route_config.comment())
                .await;
            assert!(
                existing_dns.is_ok(),
                "Failed to get DNS record for '{}' after destroy: {:?}",
                route_config.route,
                existing_dns
            );
            assert!(
                existing_dns.unwrap().is_none(),
                "DNS record for '{}' still exists after destroy.",
                route_config.route
            );
        }

        // Verify Worker route is gone
        for route_config in &res.zone_resources.routes {
            let existing_route = cloudflare_api
                .get_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name.clone()),
                    route_config.script.clone(),
                )
                .await;
            assert!(
                existing_route.is_ok(),
                "Failed to get worker route for pattern '{}' after destroy: {:?}",
                route_config.route,
                existing_route
            );
            assert!(
                existing_route.unwrap().is_none(),
                "Worker route for pattern '{}' still exists after destroy.",
                route_config.route
            );
        }

        // After destroy
        let current_rules = cloudflare_api
            .get_ruleset_rules(zone_id.clone(), ruleset_id.clone())
            .await
            .unwrap();
        assert!(
            current_rules.is_empty(),
            "Cache rules should be empty after destroy"
        );
    }
}
