use std::env;

use crate::deploy::cf_api::AccountCloudflareApi;
use crate::deploy::console_notify::ConsoleNotifier;

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
pub struct LinkupCfResources {
    worker_script_name: String,
    worker_script_parts: Vec<WorkerScriptPart>,
    kv_name: String,
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

pub trait CloudflareApi {
    async fn get_worker_script_content(&self, script_name: String) -> Result<String, DeployError>;
    async fn get_worker_script_info(
        &self,
        script_name: String,
    ) -> Result<Option<WorkerScriptInfo>, DeployError>;
    async fn create_worker_script(
        &self,
        script_name: String,
        metadata: WorkerMetadata,
        parts: Vec<WorkerScriptPart>,
    ) -> Result<(), DeployError>;
    async fn remove_worker_script(&self, script_name: String) -> Result<(), DeployError>;

    async fn get_kv_namespace_id(
        &self,
        namespace_name: String,
    ) -> Result<Option<String>, DeployError>;
    async fn create_kv_namespace(&self, namespace_id: String) -> Result<String, DeployError>;
    async fn remove_kv_namespace(&self, namespace_id: String) -> Result<(), DeployError>;
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

pub async fn deploy(account_id: &str, zone_ids: &[String]) -> Result<(), DeployError> {
    println!("Deploying to Cloudflare...");
    println!("Account ID: {}", account_id);
    println!("Zone IDs: {:?}", zone_ids);

    let api_key = env::var("CLOUDFLARE_API_KEY").expect("Missing Cloudflare API token");
    let zone_ids_strings: Vec<String> = zone_ids.iter().map(|s| s.to_string()).collect();

    let cloudflare_api =
        AccountCloudflareApi::new(account_id.to_string(), zone_ids_strings, api_key);
    let notifier = ConsoleNotifier::new();

    let resources = LinkupCfResources {
        worker_script_name: "linkup-integration-test-script".to_string(),
        worker_script_parts: vec![WorkerScriptPart {
            name: "index.js".to_string(),
            data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
        }],
        kv_name: "linkup-integration-test-kv".to_string(),
    };

    deploy_to_cloudflare(&resources, &cloudflare_api, &notifier).await?;

    Ok(())
}

async fn deploy_to_cloudflare(
    resources: &LinkupCfResources,
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    let script_name = &resources.worker_script_name;

    let existing_info = api.get_worker_script_info(script_name.clone()).await?;
    let needs_upload = if let Some(_) = existing_info {
        // The script exists, check if the content matches
        let existing_content = api.get_worker_script_content(script_name.clone()).await?;
        existing_content != LOCAL_SCRIPT_CONTENT
    } else {
        // Script doesn't exist, we need to create it
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
        }
    } else {
        notifier.notify("No changes needed. Worker script is already up to date.");
    }

    // Now handle KV namespace
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

    Ok(())
}

pub async fn destroy_from_cloudflare(
    resources: &LinkupCfResources,
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    // Remove the worker script
    api.remove_worker_script(resources.worker_script_name.to_string())
        .await?;
    notifier.notify("Worker script removed successfully.");

    // Remove the KV namespace if it exists
    let kv_name = &resources.kv_name;
    let kv_ns_id = api.get_kv_namespace_id(kv_name.clone()).await?;
    if let Some(ns_id) = kv_ns_id {
        api.remove_kv_namespace(ns_id.clone()).await?;
        notifier.notify(&format!("KV namespace '{}' removed successfully.", kv_name));
    } else {
        notifier.notify(&format!(
            "KV namespace '{}' does not exist, nothing to remove.",
            kv_name
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct TestCloudflareApi {
        pub existing_info: Option<WorkerScriptInfo>,
        pub existing_content: Option<String>,
        pub create_called_with: RefCell<Option<(String, WorkerMetadata, Vec<WorkerScriptPart>)>>,
    }

    impl CloudflareApi for TestCloudflareApi {
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

    fn test_resources() -> LinkupCfResources {
        LinkupCfResources {
            worker_script_name: "linkup-integration-test-script".to_string(),
            worker_script_parts: vec![WorkerScriptPart {
                name: "index.js".to_string(),
                data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
            }],
            kv_name: "linkup-integration-test-kv".to_string(),
        }
    }

    #[tokio::test]
    async fn test_deploy_to_cloudflare_creates_script_when_none_exists() {
        let api = TestCloudflareApi {
            existing_info: None,
            existing_content: None,
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        // Call deploy_to_cloudflare directly since deploy is async and just a wrapper
        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Worker script uploaded successfully.");

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
    async fn test_deploy_to_cloudflare_creates_script_when_content_differs() {
        let api = TestCloudflareApi {
            existing_info: Some(WorkerScriptInfo {
                id: "script-id".to_string(),
            }),
            existing_content: Some("old content".to_string()),
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Worker script uploaded successfully.");

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
    async fn test_deploy_to_cloudflare_no_changes_when_content_matches() {
        let api = TestCloudflareApi {
            existing_info: Some(WorkerScriptInfo {
                id: "script-id".to_string(),
            }),
            existing_content: Some(LOCAL_SCRIPT_CONTENT.to_string()),
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[0],
            "No changes needed. Worker script is already up to date."
        );

        assert_eq!(*notifier.confirmations_asked.borrow(), 0);

        let created = api.create_called_with.borrow();
        assert!(created.is_none());
    }

    #[tokio::test]
    async fn test_deploy_to_cloudflare_canceled_by_user() {
        let api = TestCloudflareApi {
            existing_info: None,
            existing_content: None,
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: false,
            confirmations_asked: RefCell::new(0),
        };

        let result = deploy_to_cloudflare(&test_resources(), &api, &notifier).await;
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Deployment canceled by user.");

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        assert!(created.is_none());
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
        let cloudflare_api = AccountCloudflareApi::new(
            account_id.clone(),
            vec!["test-zone-id".to_string()],
            api_key,
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
            "Failed to get kv namespace info: {:?}",
            kv_ns_id
        );
        assert!(
            kv_ns_id.unwrap().is_some(),
            "KV namespace not found after deploy."
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
            "Failed to get kv namespace after destroy: {:?}",
            kv_ns_id
        );
        assert!(
            kv_ns_id.unwrap().is_none(),
            "KV namespace still exists after destroy"
        );
    }
}
