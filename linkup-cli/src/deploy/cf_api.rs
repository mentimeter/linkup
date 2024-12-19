use reqwest::{multipart, Client};
use serde::Deserialize;
use serde_json::json;

use super::{
    cf_deploy::{CloudflareApi, WorkerMetadata, WorkerScriptInfo, WorkerScriptPart},
    DeployError,
};

#[derive(Deserialize, Debug)]
struct CloudflareWorkerScript {
    id: String,
    // Other fields omitted for brevity.
    // According to Cloudflare docs, we might have: created_on, modified_on, etc.
    #[serde(rename = "created_on")]
    created_on: String,
}

#[derive(Deserialize, Debug)]
struct CloudflareListWorkersResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<Vec<CloudflareWorkerScript>>,
}

#[derive(Deserialize, Debug)]
struct CloudflareErrorInfo {
    code: Option<u32>,
    message: String,
}

#[derive(Deserialize, Debug)]
struct KvNamespace {
    id: String,
    title: String,
}

#[derive(Deserialize, Debug)]
struct ListKvNamespacesResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<Vec<KvNamespace>>,
}

#[derive(Deserialize, Debug)]
struct CreateKvNamespaceResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<KvNamespace>,
}

/// Download Worker -> returns raw script content
/// Cloudflare docs: GET /accounts/{account_id}/workers/scripts/{script_name}
/// Returns the raw worker script text if successful.
#[derive(Deserialize, Debug)]
struct CloudflareApiResponse<T> {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<T>,
}

pub struct AccountCloudflareApi {
    account_id: String,
    zone_ids: Vec<String>,
    api_key: String,
    client: Client,
}

impl AccountCloudflareApi {
    pub fn new(account_id: String, zone_ids: Vec<String>, api_key: String) -> Self {
        let client = Client::new();
        Self {
            account_id,
            zone_ids,
            api_key,
            client,
        }
    }

    fn key_header(&self) -> (String, String) {
        (
            "Authorization".to_string(),
            format!("Bearer {}", self.api_key),
        )
    }
}

impl CloudflareApi for AccountCloudflareApi {
    async fn get_worker_script_content(&self, script_name: String) -> Result<String, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}",
            self.account_id, script_name
        );

        let resp = self
            .client
            .get(&url)
            .header(self.key_header().0, self.key_header().1)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let text = resp.text().await?;
        Ok(text)
    }

    async fn get_worker_script_info(
        &self,
        script_name: String,
    ) -> Result<Option<WorkerScriptInfo>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts",
            self.account_id
        );

        let resp = self
            .client
            .get(&url)
            .header(self.key_header().0, self.key_header().1)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let data: CloudflareListWorkersResponse = resp.json().await?;

        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(scripts) = data.result {
            for script in scripts {
                if script.id == script_name {
                    return Ok(Some(WorkerScriptInfo { id: script.id }));
                }
            }
        }
        Ok(None)
    }

    async fn create_worker_script(
        &self,
        script_name: String,
        metadata: WorkerMetadata,
        parts: Vec<WorkerScriptPart>,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}",
            self.account_id, script_name
        );

        // Prepare metadata JSON
        let bindings_json: Vec<serde_json::Value> = metadata
            .bindings
            .iter()
            .map(|b| {
                json!({
                    "type": b.type_,
                    "name": b.name,
                    "text": b.text
                })
            })
            .collect();

        let metadata_json = json!({
            "main_module": metadata.main_module,
            "compatibility_date": metadata.compatibility_date,
            "bindings": bindings_json,
        })
        .to_string();

        // Create multipart form
        let mut form = multipart::Form::new();

        form = form.text("metadata", metadata_json);

        for part in parts {
            form = form.part(
                part.name.clone(),
                multipart::Part::bytes(part.data)
                    .file_name(part.name.clone()) // not strictly required
                    .mime_str("application/javascript+module")
                    .unwrap(),
            );
        }

        let resp = self
            .client
            .put(&url)
            .header(self.key_header().0, self.key_header().1)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        // Optionally, parse the response to confirm success
        let result_data: CloudflareApiResponse<serde_json::Value> = resp.json().await?;

        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn remove_worker_script(&self, script_name: String) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}",
            self.account_id, script_name
        );

        let resp = self
            .client
            .delete(&url)
            .header(self.key_header().0, self.key_header().1)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let result_data: CloudflareApiResponse<serde_json::Value> = resp.json().await?;
        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn get_kv_namespace_id(
        &self,
        namespace_name: String,
    ) -> Result<Option<String>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces",
            self.account_id
        );

        let resp = self
            .client
            .get(&url)
            .header(self.key_header().0, self.key_header().1)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let data: ListKvNamespacesResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(namespaces) = data.result {
            for ns in namespaces {
                if ns.title == namespace_name {
                    return Ok(Some(ns.id));
                }
            }
        }
        Ok(None)
    }

    async fn create_kv_namespace(&self, namespace_name: String) -> Result<String, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces",
            self.account_id
        );

        let body = serde_json::json!({
            "title": namespace_name
        });

        let resp = self
            .client
            .post(&url)
            .header(self.key_header().0, self.key_header().1)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let data: CreateKvNamespaceResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(ns) = data.result {
            Ok(ns.id)
        } else {
            Err(DeployError::UnexpectedResponse(
                "No namespace ID returned".to_string(),
            ))
        }
    }

    async fn remove_kv_namespace(&self, namespace_id: String) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces/{}",
            self.account_id, namespace_id
        );

        let resp = self
            .client
            .delete(&url)
            .header(self.key_header().0, self.key_header().1)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let result_data: CloudflareApiResponse<serde_json::Value> = resp.json().await?;
        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }
}
