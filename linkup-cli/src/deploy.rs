use std::cell::RefCell;
use std::rc::Rc;

#[derive(thiserror::Error, Debug)]
pub enum DeployError {
    #[error("Failed to deploy to Cloudflare")]
    CloudflareError,
}

#[derive(Debug, Clone)]
struct WorkerScriptInfo {
    created_at: String,
}

struct WorkerMetadata {
    main_module: String,
    bindings: Vec<WorkerBinding>,
    compatibility_date: String,
}

struct WorkerScriptPart {
    name: String,
    data: Vec<u8>,
}

struct WorkerBinding {
    type_: String,
    name: String,
    text: String,
}

trait CloudflareApi {
    fn get_worker_script_content(&self, script_name: String) -> Result<String, DeployError>;
    fn get_worker_script_info(
        &self,
        script_name: String,
    ) -> Result<Option<WorkerScriptInfo>, DeployError>;
    fn create_worker_script(
        &self,
        script_name: String,
        metadata: WorkerMetadata,
        parts: Vec<WorkerScriptPart>,
    ) -> Result<(), DeployError>;
}

pub trait DeployNotifier {
    fn ask_confirmation(&self) -> bool;
    fn notify(&self, message: &str);
}

const LOCAL_SCRIPT_CONTENT: &str = r#"addEventListener('fetch', event => {
    event.respondWith(new Response("Hello from the new Worker!", {status: 200}));
});"#;

pub async fn deploy(account_id: &str, zone_ids: &[String]) -> Result<(), DeployError> {
    println!("Deploying to Cloudflare...");
    println!("Account ID: {}", account_id);
    println!("Zone IDs: {:?}", zone_ids);

    // Here we might call deploy_to_cloudflare with a real implementation.
    // For example:
    // let cloudflare_api = RealCloudflareApi::new(api_token, ...);
    // let notifier = RealNotifier::new(...);
    // deploy_to_cloudflare(account_id, zone_ids, &cloudflare_api, &notifier)?;

    Ok(())
}

fn deploy_to_cloudflare(
    account_id: &str,
    zone_ids: &[String],
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    let script_name = "my-worker-script".to_string();

    let existing_info = api.get_worker_script_info(script_name.clone())?;
    let needs_upload = if let Some(_) = existing_info {
        // The script exists, check if the content matches
        let existing_content = api.get_worker_script_content(script_name.clone())?;
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
                compatibility_date: "2023-01-01".to_string(),
            };

            let parts = vec![WorkerScriptPart {
                name: "index.js".to_string(),
                data: LOCAL_SCRIPT_CONTENT.as_bytes().to_vec(),
            }];

            api.create_worker_script(script_name, metadata, parts)?;
            notifier.notify("Worker script uploaded successfully.");
        } else {
            notifier.notify("Deployment canceled by user.");
        }
    } else {
        notifier.notify("No changes needed. Worker script is already up to date.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCloudflareApi {
        pub existing_info: Option<WorkerScriptInfo>,
        pub existing_content: Option<String>,
        pub create_called_with: RefCell<Option<(String, WorkerMetadata, Vec<WorkerScriptPart>)>>,
    }

    impl CloudflareApi for TestCloudflareApi {
        fn get_worker_script_content(&self, script_name: String) -> Result<String, DeployError> {
            if let Some(content) = &self.existing_content {
                Ok(content.clone())
            } else {
                Err(DeployError::CloudflareError)
            }
        }

        fn get_worker_script_info(
            &self,
            _script_name: String,
        ) -> Result<Option<WorkerScriptInfo>, DeployError> {
            Ok(self.existing_info.clone())
        }

        fn create_worker_script(
            &self,
            script_name: String,
            metadata: WorkerMetadata,
            parts: Vec<WorkerScriptPart>,
        ) -> Result<(), DeployError> {
            *self.create_called_with.borrow_mut() = Some((script_name, metadata, parts));
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

    #[test]
    fn test_deploy_to_cloudflare_creates_script_when_none_exists() {
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

        let account_id = "test-account";
        let zone_ids = vec!["zone1".to_string()];

        // Call deploy_to_cloudflare directly since deploy is async and just a wrapper
        let result = deploy_to_cloudflare(account_id, &zone_ids, &api, &notifier);
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Worker script uploaded successfully.");

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "my-worker-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 0);
        assert_eq!(metadata.compatibility_date, "2023-01-01");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "index.js");
        assert_eq!(
            String::from_utf8(parts[0].data.clone()).unwrap(),
            LOCAL_SCRIPT_CONTENT
        );
    }

    #[test]
    fn test_deploy_to_cloudflare_creates_script_when_content_differs() {
        let api = TestCloudflareApi {
            existing_info: Some(WorkerScriptInfo {
                created_at: "2023-01-01T00:00:00Z".to_string(),
            }),
            existing_content: Some("old content".to_string()),
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let account_id = "test-account";
        let zone_ids = vec!["zone1".to_string()];

        let result = deploy_to_cloudflare(account_id, &zone_ids, &api, &notifier);
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Worker script uploaded successfully.");

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        let (script_name, metadata, parts) = created.as_ref().unwrap();

        assert_eq!(script_name, "my-worker-script");
        assert_eq!(metadata.main_module, "index.js");
        assert_eq!(metadata.bindings.len(), 0);
        assert_eq!(metadata.compatibility_date, "2023-01-01");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "index.js");
        assert_eq!(
            String::from_utf8(parts[0].data.clone()).unwrap(),
            LOCAL_SCRIPT_CONTENT
        );
    }

    #[test]
    fn test_deploy_to_cloudflare_no_changes_when_content_matches() {
        let api = TestCloudflareApi {
            existing_info: Some(WorkerScriptInfo {
                created_at: "2023-01-01T00:00:00Z".to_string(),
            }),
            existing_content: Some(LOCAL_SCRIPT_CONTENT.to_string()),
            create_called_with: RefCell::new(None),
        };

        let notifier = TestNotifier {
            messages: RefCell::new(vec![]),
            confirmation_response: true,
            confirmations_asked: RefCell::new(0),
        };

        let account_id = "test-account";
        let zone_ids = vec!["zone1".to_string()];

        let result = deploy_to_cloudflare(account_id, &zone_ids, &api, &notifier);
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0],
            "No changes needed. Worker script is already up to date."
        );

        assert_eq!(*notifier.confirmations_asked.borrow(), 0);

        let created = api.create_called_with.borrow();
        assert!(created.is_none());
    }

    #[test]
    fn test_deploy_to_cloudflare_canceled_by_user() {
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

        let account_id = "test-account";
        let zone_ids = vec!["zone1".to_string()];

        let result = deploy_to_cloudflare(account_id, &zone_ids, &api, &notifier);
        assert!(result.is_ok());

        let messages = notifier.messages.borrow();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], "Worker script differs or does not exist.");
        assert_eq!(messages[1], "Deployment canceled by user.");

        assert_eq!(*notifier.confirmations_asked.borrow(), 1);

        let created = api.create_called_with.borrow();
        assert!(created.is_none());
    }
}
