use crate::commands::deploy::auth;
use crate::commands::deploy::resources::cf_resources;

use super::api::{AccountCloudflareApi, CloudflareApi};
use super::console_notify::ConsoleNotifier;
use super::resources::TargetCfResources;

#[derive(thiserror::Error, Debug)]
pub enum DeployError {
    #[error("No authentication method found, please set CLOUDFLARE_API_KEY and CLOUDFLARE_EMAIL or CLOUDFLARE_API_TOKEN")]
    NoAuthenticationError,
    #[error("Cloudflare API error: {0}")]
    CloudflareApiError(#[from] reqwest::Error),
    #[error("Cloudflare Client error: {0}")]
    CloudflareClientError(#[from] cloudflare::framework::response::ApiFailure),
    #[error("Unexpected Cloudflare API response: {0}")]
    UnexpectedResponse(String),
    #[error("Other failure")]
    OtherError,
}

pub trait DeployNotifier {
    fn ask_confirmation(&self) -> bool;
    fn notify(&self, message: &str);
}

#[derive(clap::Args)]
pub struct DeployArgs {
    #[arg(short = 'e', long = "email", help = "Cloudflare user email")]
    email: String,

    #[arg(short = 'k', long = "api-key", help = "Cloudflare user global API Key")]
    api_key: String,

    #[arg(short = 'a', long = "account-id", help = "Cloudflare account ID")]
    account_id: String,

    #[arg(
        short = 'z',
        long = "zone-ids",
        help = "Cloudflare zone IDs",
        num_args = 1..,
        required = true
    )]
    zone_ids: Vec<String>,
}

pub async fn deploy(args: &DeployArgs) -> Result<(), DeployError> {
    println!("Deploying to Cloudflare...");
    println!("Account ID: {}", args.account_id);
    println!("Zone IDs: {:?}", args.zone_ids);

    let auth = auth::CloudflareGlobalTokenAuth::new(args.api_key.clone(), args.email.clone());
    let zone_ids_strings: Vec<String> = args.zone_ids.iter().map(|s| s.to_string()).collect();

    // TODO(augustoccesar)[2025-02-19]: Move functionality to use Cloudflare module client instead of AccountCloudflareApi.
    let cloudflare_api = AccountCloudflareApi::new(
        args.account_id.to_string(),
        zone_ids_strings.clone(),
        Box::new(auth),
    );
    let cloudflare_client = cloudflare::framework::async_api::Client::new(
        cloudflare::framework::auth::Credentials::UserAuthKey {
            email: args.email.clone(),
            key: args.api_key.clone(),
        },
        cloudflare::framework::HttpApiClientConfig::default(),
        cloudflare::framework::Environment::Production,
    )
    .expect("Cloudflare API Client to have been created");
    let notifier = ConsoleNotifier::new();

    let mut zone_names = Vec::with_capacity(zone_ids_strings.len());
    for zone_id in zone_ids_strings {
        let zone_name = cloudflare_api.get_zone_name(&zone_id).await?;
        zone_names.push(zone_name);
    }

    let resources = cf_resources(
        args.account_id.clone(),
        args.zone_ids[0].clone(),
        &zone_names,
        &args.zone_ids,
    );

    deploy_to_cloudflare(&resources, &cloudflare_api, &cloudflare_client, &notifier).await?;

    Ok(())
}

pub async fn deploy_to_cloudflare(
    resources: &TargetCfResources,
    api: &impl CloudflareApi,
    cloudflare_client: &cloudflare::framework::async_api::Client,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    // 1) Check what needs to change
    let plan = resources.check_deploy_plan(api, cloudflare_client).await?;

    // 2) If nothing changed, we can just early-out
    if plan.is_empty() {
        notifier.notify("No changes needed. Cloudflare resources are already up to date.");
        return Ok(());
    }

    // 3) Otherwise, show some summary to the user and ask for confirmation
    notifier.notify("The following changes are needed:");
    // (You could display them in a fancy way. Here we just do a debug dump.)
    notifier.notify(&format!("{:#?}", plan));

    if !notifier.ask_confirmation() {
        notifier.notify("Deployment canceled by user.");
        return Ok(());
    }

    // 4) Execute the plan
    notifier.notify("Applying changes to Cloudflare...");
    resources
        .execute_deploy_plan(api, cloudflare_client, &plan, notifier)
        .await?;
    notifier.notify("Deployment complete.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use cloudflare::framework::{
        async_api::Client, auth, endpoint::spec::EndpointSpec, Environment, HttpApiClientConfig,
    };
    use mockito::ServerGuard;
    use std::cell::RefCell;

    use crate::commands::deploy::{
        self,
        api::Token,
        cf_destroy::destroy_from_cloudflare,
        resources::{
            rules_equal, DNSRecord, KvNamespace, Rule, TargectCfZoneResources, TargetCacheRules,
            TargetDNSRecord, TargetWorkerRoute, WorkerBinding, WorkerMetadata, WorkerScriptInfo,
            WorkerScriptPart, LINKUP_ACCOUNT_TOKEN_NAME,
        },
    };

    use super::*;

    fn test_client(mock_server_url: String) -> Client {
        let mock_server_url = url::Url::parse(&mock_server_url).unwrap();
        Client::new(
            auth::Credentials::UserAuthKey {
                email: "test@example.com".to_string(),
                key: "test-api-key".to_string(),
            },
            HttpApiClientConfig::default(),
            Environment::Custom(mock_server_url),
        )
        .unwrap()
    }

    async fn mock_cloudflare_endpoints(
        mock_server: &mut ServerGuard,
        resources: &TargetCfResources,
    ) {
        let req = cloudflare::endpoints::workers::ListBindings {
            account_id: &resources.account_id,
            script_name: &resources.worker_script_name,
        };

        let res = serde_json::to_string(&cloudflare::framework::response::ApiSuccess::<Vec<()>> {
            result: vec![],
            result_info: None,
            messages: serde_json::json!([]),
            errors: vec![],
        })
        .unwrap();

        let path = format!("/{}", req.path().to_string().as_str());

        mock_server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_body(res)
            .create_async()
            .await;
    }

    const LOCAL_SCRIPT_CONTENT: &str = r#"
export default {
	async fetch(request, env, ctx) {
		return new Response('Hello World!');
	},
};
"#;

    struct TestCloudflareApi {
        zone_ids: Vec<String>,

        pub existing_info: Option<WorkerScriptInfo>,
        pub create_called_with: RefCell<Option<(String, WorkerMetadata, Vec<WorkerScriptPart>)>>,

        pub dns_records: RefCell<Vec<DNSRecord>>,
        pub worker_routes: RefCell<Vec<(String, String, String)>>,
        pub account_tokens: RefCell<Vec<(String, String)>>,
    }

    impl TestCloudflareApi {
        fn new(zone_ids: Vec<String>) -> Self {
            Self {
                zone_ids,
                existing_info: None,
                create_called_with: RefCell::new(None),
                dns_records: RefCell::new(vec![]),
                worker_routes: RefCell::new(vec![]),
                account_tokens: RefCell::new(vec![]),
            }
        }
    }

    impl CloudflareApi for TestCloudflareApi {
        fn zone_ids(&self) -> &Vec<String> {
            &self.zone_ids
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

        async fn remove_worker_script(&self, _script_name: String) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_kv_namespace_id(
            &self,
            _namespace_name: String,
        ) -> Result<Option<String>, DeployError> {
            Ok(Some("existing-namespace-id".to_string()))
        }

        async fn create_kv_namespace(&self, _namespace_id: String) -> Result<String, DeployError> {
            Ok("new-namespace-id".to_string())
        }

        async fn remove_kv_namespace(&self, _namespace_id: String) -> Result<(), DeployError> {
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

        async fn get_account_worker_subdomain(&self) -> Result<Option<String>, DeployError> {
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
                .retain(|(z, p, _s)| !(z == &zone_id && format!("route-id-for-{}", p) == route_id));
            Ok(())
        }

        async fn get_zone_name(&self, _zone_id: &str) -> Result<String, DeployError> {
            Ok("example.com".to_string())
        }

        async fn get_ruleset(
            &self,
            _zone_id: String,
            _name: String,
            _phase: String,
        ) -> Result<Option<String>, DeployError> {
            Ok(None)
        }

        async fn create_ruleset(
            &self,
            _zone_id: String,
            _name: String,
            _phase: String,
        ) -> Result<String, DeployError> {
            Ok("new-ruleset-id".to_string())
        }

        async fn update_ruleset_rules(
            &self,
            _zone_id: String,
            _ruleset: String,
            _rules: Vec<Rule>,
        ) -> Result<(), DeployError> {
            Ok(())
        }

        async fn remove_ruleset_rules(
            &self,
            _zone_id: String,
            _ruleset: String,
        ) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_ruleset_rules(
            &self,
            _zone_id: String,
            _ruleset: String,
        ) -> Result<Vec<Rule>, DeployError> {
            Ok(vec![])
        }

        async fn get_worker_script_version(
            &self,
            _script_name: String,
        ) -> Result<Option<String>, DeployError> {
            Ok(None)
        }

        async fn create_account_token(&self, _name: &str) -> Result<String, DeployError> {
            let token_name = LINKUP_ACCOUNT_TOKEN_NAME.to_string();
            self.account_tokens
                .borrow_mut()
                .push(("created".to_string(), token_name.clone()));
            Ok(token_name)
        }

        async fn list_account_tokens(&self) -> Result<Vec<deploy::api::Token>, DeployError> {
            let tokens = self
                .account_tokens
                .clone()
                .into_inner()
                .iter()
                .map(|(id, name)| Token {
                    id: id.clone(),
                    name: Some(name.clone()),
                    value: None,
                })
                .collect();

            Ok(tokens)
        }

        async fn remove_account_token(&self, _id: &str) -> Result<(), DeployError> {
            Ok(())
        }

        async fn get_worker_subdomain(
            &self,
            _script_name: String,
        ) -> Result<deploy::api::WorkerSubdomain, DeployError> {
            Ok(deploy::api::WorkerSubdomain { enabled: true })
        }

        async fn post_worker_subdomain(
            &self,
            _script_name: String,
            _enabled: bool,
            _previews_enabled: Option<bool>,
        ) -> Result<(), DeployError> {
            Ok(())
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
            account_id: "acc_id_123".to_string(),
            worker_script_name: "linkup-integration-test-script".to_string(),
            worker_script_entry: "index.js".to_string(),
            worker_script_parts: vec![WorkerScriptPart {
                name: "index.js".to_string(),
                data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
                content_type: "application/javascript+module".to_string(),
            }],
            worker_script_bindings: vec![WorkerBinding::PlainText {
                name: "INTEGRATION_TEST_ARG".to_string(),
                text: "plain_text".to_string(),
            }],
            worker_script_schedules: vec![cloudflare::endpoints::workers::WorkersSchedule {
                cron: Some("0 12 * * 2-6".to_string()),
                ..Default::default()
            }],
            kv_namespaces: vec![KvNamespace {
                name: "linkup-integration-test-kv".to_string(),
                binding: "LINKUP_SESSIONS".to_string(),
            }],
            zone_resources: TargectCfZoneResources {
                dns_records: vec![TargetDNSRecord {
                    route: "linkup-integration-test".to_string(),
                    script: "linkup-integration-test-script".to_string(),
                }],
                routes: vec![TargetWorkerRoute {
                    route: "linkup-integration-test.".to_string(),
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

        let resources = test_resources();

        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
        assert!(result.is_ok());

        // Check that a worker script was created
        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "linkup-integration-test-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 4);
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

        let resources = test_resources();
        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
        assert!(result.is_ok());

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "linkup-integration-test-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 4);
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

        let resources = test_resources();
        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
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

        let resources = test_resources();
        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
        assert!(result.is_ok());

        // Check DNS record created
        let dns_records = api.dns_records.borrow();
        assert_eq!(dns_records.len(), 1);
        assert_eq!(dns_records[0].name, "linkup-integration-test");
        assert!(dns_records[0]
            .content
            .contains("linkup-integration-test-script.workers.dev"));

        // Check route created
        let routes = api.worker_routes.borrow();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].1, "linkup-integration-test.example.com/*");
        assert_eq!(routes[0].2, "linkup-integration-test-script");
    }

    #[tokio::test]
    async fn test_deploy_creates_account_token() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let resources = test_resources();
        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
        assert!(result.is_ok());

        let tokens = api.account_tokens.borrow();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "created".to_string());
        assert_eq!(tokens[0].1, LINKUP_ACCOUNT_TOKEN_NAME.to_string());
    }

    #[tokio::test]
    async fn test_deploy_skips_creating_account_token_if_already_exists() {
        let api = TestCloudflareApi::new(vec!["test-zone-id".to_string()]);
        api.account_tokens.borrow_mut().push((
            "existing".to_string(),
            LINKUP_ACCOUNT_TOKEN_NAME.to_string(),
        ));

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let resources = test_resources();
        let mut mock_server = mockito::Server::new_async().await;
        let client = test_client(mock_server.url());
        mock_cloudflare_endpoints(&mut mock_server, &resources).await;

        // Call deploy_to_cloudflare directly
        let result = deploy_to_cloudflare(&resources, &api, &client, &notifier).await;
        assert!(result.is_ok());

        let tokens = api.account_tokens.borrow();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "existing".to_string());
        assert_eq!(tokens[0].1, LINKUP_ACCOUNT_TOKEN_NAME.to_string());
    }

    #[tokio::test]
    async fn test_deploy_and_destroy_real_integration() {
        // Skip test if environment variables aren't set
        let account_id = match std::env::var("CLOUDFLARE_ACCOUNT_ID") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Skipping test: CLOUDFLARE_ACCOUNT_ID is not set.");
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
            eprintln!("Skipping test: LINKUP_DEPLOY_INTEGRATION_TEST is not set to 'true'.");
            return;
        }

        // Set up Cloudflare API
        let api_key = std::env::var("CLOUDFLARE_API_KEY").expect("CLOUDFLARE_API_KEY is not set");
        let email = std::env::var("CLOUDFLARE_EMAIL").expect("CLOUDFLARE_EMAIL is not set");

        let global_api_auth =
            deploy::auth::CloudflareGlobalTokenAuth::new(api_key.clone(), email.clone());
        let cloudflare_api = AccountCloudflareApi::new(
            account_id.clone(),
            vec![zone_id.to_string()],
            Box::new(global_api_auth),
        );

        let cloudflare_client = cloudflare::framework::async_api::Client::new(
            cloudflare::framework::auth::Credentials::UserAuthKey {
                email,
                key: api_key,
            },
            cloudflare::framework::HttpApiClientConfig::default(),
            cloudflare::framework::Environment::Production,
        )
        .expect("Cloudflare API Client to have been created");

        let notifier = ConsoleNotifier::new();

        // Deploy the resources
        let res = test_resources();
        let result =
            deploy_to_cloudflare(&res, &cloudflare_api, &cloudflare_client, &notifier).await;
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
        for kv_namespace in &res.kv_namespaces {
            let kv_ns_id = cloudflare_api
                .get_kv_namespace_id(kv_namespace.name.clone())
                .await;
            assert!(
                kv_ns_id.is_ok(),
                "Failed to get KV namespace info: {:?}",
                kv_ns_id
            );
            assert!(
                kv_ns_id.unwrap().is_some(),
                "KV namespace not found after deploy."
            );
        }

        // Verify worker subdomain
        let worker_subdomain = cloudflare_api
            .get_worker_subdomain(script_name.clone())
            .await;

        assert!(
            worker_subdomain.is_ok(),
            "Failed to get worker subdomain info: {:?}",
            worker_subdomain
        );
        assert!(
            worker_subdomain.unwrap().enabled,
            "Worker subdomain should be enabled after deploy."
        );

        // Verify DNS record exists
        for dns_record in &res.zone_resources.dns_records {
            let existing_dns = cloudflare_api
                .get_dns_record(zone_id.clone(), dns_record.comment())
                .await;
            assert!(
                existing_dns.is_ok(),
                "Failed to get DNS record for '{}': {:?}",
                dns_record.route,
                existing_dns
            );
            assert!(
                existing_dns.unwrap().is_some(),
                "DNS record for '{}' not found after deploy.",
                dns_record.route
            );
        }

        // Verify Worker route exists
        let zone_name = cloudflare_api.get_zone_name(&zone_id).await.unwrap();
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

        // Verify account token
        let tokens = cloudflare_api.list_account_tokens().await.unwrap();
        let linkup_account_token = tokens
            .iter()
            .find(|token| token.name.as_deref() == Some(LINKUP_ACCOUNT_TOKEN_NAME));
        assert!(
            linkup_account_token.is_some(),
            "Linkup account token should exist after deploy"
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
        for kv_namespace in &res.kv_namespaces {
            let kv_ns_id = cloudflare_api
                .get_kv_namespace_id(kv_namespace.name.clone())
                .await;
            assert!(
                kv_ns_id.is_ok(),
                "Failed to get KV namespace after destroy: {:?}",
                kv_ns_id
            );
            assert!(
                kv_ns_id.unwrap().is_none(),
                "KV namespace still exists after destroy"
            );
        }

        // Verify DNS record is gone
        for dns_record in &res.zone_resources.dns_records {
            let existing_dns = cloudflare_api
                .get_dns_record(zone_id.clone(), dns_record.comment())
                .await;
            assert!(
                existing_dns.is_ok(),
                "Failed to get DNS record for '{}' after destroy: {:?}",
                dns_record.route,
                existing_dns
            );
            assert!(
                existing_dns.unwrap().is_none(),
                "DNS record for '{}' still exists after destroy.",
                dns_record.route
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

        // Verify account token is gone
        let tokens = cloudflare_api.list_account_tokens().await.unwrap();
        let linkup_account_token = tokens
            .iter()
            .find(|token| token.name.as_deref() == Some(LINKUP_ACCOUNT_TOKEN_NAME));
        assert!(
            linkup_account_token.is_none(),
            "Linkup account token '{}' still exists after destroy.",
            linkup_account_token.unwrap().id,
        );
    }
}
