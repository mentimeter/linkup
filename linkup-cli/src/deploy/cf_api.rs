use reqwest::{header::HeaderMap, multipart, Client};
use serde::Deserialize;
use serde_json::json;

use super::{
    cf_auth::CloudflareApiAuth,
    cf_deploy::{DNSRecord, WorkerMetadata, WorkerScriptInfo, WorkerScriptPart},
    DeployError,
};

pub trait CloudflareApi {
    fn zone_ids(&self) -> &Vec<String>;

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

    async fn get_zone_name(&self, zone_id: String) -> Result<String, DeployError>;

    async fn get_dns_record(
        &self,
        zone_id: String,
        comment: String,
    ) -> Result<Option<DNSRecord>, DeployError>;
    async fn create_dns_record(
        &self,
        zone_id: String,
        record: DNSRecord,
    ) -> Result<(), DeployError>;
    async fn remove_dns_record(
        &self,
        zone_id: String,
        record_id: String,
    ) -> Result<(), DeployError>;

    async fn get_worker_subdomain(&self) -> Result<Option<String>, DeployError>;

    async fn get_worker_route(
        &self,
        zone_id: String,
        pattern: String,
        script_name: String,
    ) -> Result<Option<String>, DeployError>;
    async fn create_worker_route(
        &self,
        zone_id: String,
        pattern: String,
        script_name: String,
    ) -> Result<(), DeployError>;
    async fn remove_worker_route(
        &self,
        zone_id: String,
        route_id: String,
    ) -> Result<(), DeployError>;
}

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

#[derive(Deserialize, Debug)]
struct DnsRecordResult {
    id: String,
    name: String,
    #[serde(rename = "type")]
    record_type: String,
    content: String,
    proxied: Option<bool>,
    comment: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ListDnsRecordsResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<Vec<DnsRecordResult>>,
}

#[derive(Deserialize, Debug)]
struct CreateDnsRecordResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<DnsRecordResult>,
}

#[derive(Deserialize, Debug)]
struct WorkerRoute {
    id: String,
    pattern: String,
    script: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ListWorkerRoutesResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<Vec<WorkerRoute>>,
}

#[derive(Deserialize, Debug)]
struct CreateWorkerRouteResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<WorkerRoute>,
}

#[derive(serde::Deserialize, Debug)]
struct GetSubdomainResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<SubdomainResult>,
}

#[derive(serde::Deserialize, Debug)]
struct SubdomainResult {
    subdomain: String,
}

#[derive(Deserialize, Debug)]
struct CloudflareZoneResponse {
    success: bool,
    errors: Vec<CloudflareErrorInfo>,
    messages: Vec<CloudflareErrorInfo>,
    result: Option<ZoneInfo>,
}

#[derive(Deserialize, Debug)]
struct ZoneInfo {
    id: String,
    name: String,
    status: String,
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
    api_auth: Box<dyn CloudflareApiAuth>,
    client: Client,
}

impl AccountCloudflareApi {
    pub fn new(
        account_id: String,
        zone_ids: Vec<String>,
        api_auth: Box<dyn CloudflareApiAuth>,
    ) -> Self {
        let client = Client::new();
        Self {
            account_id,
            zone_ids,
            api_auth,
            client,
        }
    }
}

impl CloudflareApi for AccountCloudflareApi {
    fn zone_ids(&self) -> &Vec<String> {
        &self.zone_ids
    }

    async fn get_worker_script_content(&self, script_name: String) -> Result<String, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}",
            self.account_id, script_name
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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
            .headers(self.api_auth.headers())
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

    async fn get_zone_name(&self, zone_id: String) -> Result<String, DeployError> {
        let url = format!("https://api.cloudflare.com/client/v4/zones/{}", zone_id);

        let resp = self
            .client
            .get(&url)
            .headers(self.api_auth.headers())
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

        let data: CloudflareZoneResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::UnexpectedResponse(
                "Failed to retrieve zone name from Cloudflare".to_string(),
            ));
        }

        if let Some(zone_info) = data.result {
            Ok(zone_info.name)
        } else {
            Err(DeployError::UnexpectedResponse(
                "Zone name not found in response".to_string(),
            ))
        }
    }

    async fn get_dns_record(
        &self,
        zone_id: String,
        comment: String,
    ) -> Result<Option<DNSRecord>, DeployError> {
        // Assuming record_tag corresponds to DNS record name
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
            zone_id
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.api_auth.headers())
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

        let data: ListDnsRecordsResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(records) = data.result {
            if let Some(r) = records
                .into_iter()
                .find(|r| r.comment == Some(comment.clone()))
            {
                return Ok(Some(DNSRecord {
                    id: r.id,
                    name: r.name,
                    record_type: r.record_type,
                    content: r.content,
                    comment: comment.clone(),
                    proxied: r.proxied.unwrap_or(false),
                }));
            }
        }

        Ok(None)
    }

    async fn create_dns_record(
        &self,
        zone_id: String,
        record: DNSRecord,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
            zone_id
        );

        let body = serde_json::json!({
            "type": record.record_type,
            "name": record.name,
            "content": record.content,
            "proxied": record.proxied,
            "comment": record.comment
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.api_auth.headers())
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

        let result_data: CreateDnsRecordResponse = resp.json().await?;
        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn remove_dns_record(
        &self,
        zone_id: String,
        record_id: String,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
            zone_id, record_id
        );

        let resp = self
            .client
            .delete(&url)
            .headers(self.api_auth.headers())
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

    async fn get_worker_subdomain(&self) -> Result<Option<String>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/subdomain",
            self.account_id
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.api_auth.headers())
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

        let data: GetSubdomainResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        // If the request was successful, check for subdomain
        if let Some(result) = data.result {
            Ok(Some(result.subdomain))
        } else {
            // success = true but no result.subdomain returned
            Ok(None)
        }
    }

    async fn get_worker_route(
        &self,
        zone_id: String,
        pattern: String,
        script_name: String,
    ) -> Result<Option<String>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/workers/routes",
            zone_id
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.api_auth.headers())
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

        let data: ListWorkerRoutesResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(routes) = data.result {
            for route in routes {
                if route.pattern == pattern && route.script.as_deref() == Some(&script_name) {
                    return Ok(Some(route.id));
                }
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
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/workers/routes",
            zone_id
        );

        let body = serde_json::json!({
            "pattern": pattern,
            "script": script_name
        });

        let resp = self
            .client
            .post(&url)
            .headers(self.api_auth.headers())
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

        let result_data: CreateWorkerRouteResponse = resp.json().await?;
        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn remove_worker_route(
        &self,
        zone_id: String,
        route_id: String,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/workers/routes/{}",
            zone_id, route_id
        );

        let resp = self
            .client
            .delete(&url)
            .headers(self.api_auth.headers())
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
