use futures::FutureExt;
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
    api_token: String,
    client: Client,
}

impl AccountCloudflareApi {
    pub fn new(account_id: String, zone_ids: Vec<String>, api_token: String) -> Self {
        let client = Client::new();
        Self {
            account_id,
            zone_ids,
            api_token,
            client,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_token)
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
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DeployError::UnexpectedResponse(resp.status().to_string()));
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
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DeployError::UnexpectedResponse(resp.status().to_string()));
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
            "bindings": bindings_json
        })
        .to_string();

        // Create multipart form
        let mut form = multipart::Form::new().text("metadata", metadata_json);

        for part in parts {
            form = form.part(
                part.name.clone(),
                multipart::Part::bytes(part.data)
                    .file_name(part.name.clone()) // not strictly required
                    .mime_str("application/javascript")
                    .unwrap(),
            );
        }

        let resp = self
            .client
            .put(&url)
            .header("Authorization", self.auth_header())
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DeployError::UnexpectedResponse(resp.status().to_string()));
        }

        // Optionally, parse the response to confirm success
        let result_data: CloudflareApiResponse<serde_json::Value> = resp.json().await?;

        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }
}
