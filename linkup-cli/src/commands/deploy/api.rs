use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    auth::CloudflareApiAuth,
    resources::{DNSRecord, Rule, WorkerMetadata, WorkerScriptInfo, WorkerScriptPart},
    DeployError,
};

pub trait CloudflareApi {
    fn zone_ids(&self) -> &Vec<String>;

    async fn get_worker_script_info(
        &self,
        script_name: String,
    ) -> Result<Option<WorkerScriptInfo>, DeployError>;
    async fn get_worker_script_version(
        &self,
        script_name: String,
    ) -> Result<Option<String>, DeployError>;
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

    async fn get_ruleset(
        &self,
        zone_id: String,
        name: String,
        phase: String,
    ) -> Result<Option<String>, DeployError>;
    async fn create_ruleset(
        &self,
        zone_id: String,
        name: String,
        phase: String,
    ) -> Result<String, DeployError>;
    async fn get_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
    ) -> Result<Vec<Rule>, DeployError>;
    async fn update_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
        rules: Vec<Rule>,
    ) -> Result<(), DeployError>;
    async fn remove_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
    ) -> Result<(), DeployError>;
}

#[derive(Deserialize, Debug)]
struct CloudflareWorkerScript {
    id: String,
}

#[derive(Deserialize, Debug)]
struct CloudflareListWorkersResponse {
    success: bool,
    result: Option<Vec<CloudflareWorkerScript>>,
}

#[derive(Deserialize, Debug)]
struct CfListVersionsResponse {
    success: bool,
    // Only the fields we care about from the API response
    result: CfListVersionsItems,
}

#[derive(Deserialize, Debug)]
struct CfListVersionsItems {
    items: Vec<CfScriptVersionMetadata>,
}

#[derive(Deserialize, Debug)]
struct CfScriptVersionMetadata {
    /// The ID of a particular worker version, e.g., "d9a8f2cc7ed7435d90b3f2947b83673b"
    pub id: String,
    // Possibly other fields like `created_on`, etc.
}

#[derive(Deserialize, Debug)]
struct CfSingleVersionResponse {
    success: bool,
    // In some CF docs, "result" can be an object containing script+metadata
    result: CfSingleVersionResult,
}

#[derive(Deserialize, Debug)]
struct CfSingleVersionResult {
    // The actual script content is in the bindings
    resources: CfSingleVersionResources,
}

#[derive(Deserialize, Debug)]
struct CfSingleVersionResources {
    // Possibly other fields like size, usage model, etc.
    #[serde(default)]
    pub bindings: Vec<CfBinding>,
}

#[derive(Deserialize, Debug)]
struct CfBinding {
    #[serde(rename = "type")]
    pub type_: String,

    pub name: String,

    #[serde(default)]
    pub text: Option<String>,
    // Possibly other fields like namespace_id, secret text, etc.
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
    result: Option<Vec<KvNamespace>>,
}

#[derive(Deserialize, Debug)]
struct CreateKvNamespaceResponse {
    success: bool,
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
    result: Option<Vec<DnsRecordResult>>,
}

#[derive(Deserialize, Debug)]
struct CreateDnsRecordResponse {
    success: bool,
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
    result: Option<Vec<WorkerRoute>>,
}

#[derive(Deserialize, Debug)]
struct CreateWorkerRouteResponse {
    success: bool,
}

#[derive(serde::Deserialize, Debug)]
struct GetSubdomainResponse {
    success: bool,
    result: Option<SubdomainResult>,
}

#[derive(serde::Deserialize, Debug)]
struct SubdomainResult {
    subdomain: String,
}

#[derive(Deserialize, Debug)]
struct CloudflareZoneResponse {
    success: bool,
    result: Option<ZoneInfo>,
}

#[derive(Deserialize, Debug)]
struct ZoneInfo {
    name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RulesetListResponse {
    pub result: Option<Vec<ListRuleset>>,
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ListRuleset {
    pub id: String,
    pub name: String,
    pub phase: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RulesetResponse {
    pub result: Option<Ruleset>,
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Ruleset {
    pub id: String,
    pub name: String,
    pub phase: String,
    pub rules: Option<Vec<Rule>>,
    pub version: String,
}

const WORKER_VERSION_TAG: &str = "LINKUP_VERSION_TAG";

#[derive(Deserialize, Debug)]
struct CloudflareApiResponse {
    success: bool,
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
                    return Ok(Some(WorkerScriptInfo {}));
                }
            }
        }
        Ok(None)
    }

    async fn get_worker_script_version(
        &self,
        script_name: String,
    ) -> Result<Option<String>, DeployError> {
        // 1) Get the list of versions
        let list_versions_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}/versions",
            self.account_id, script_name
        );

        let resp = self
            .client
            .get(&list_versions_url)
            .headers(self.api_auth.headers())
            .send()
            .await?;

        if resp.status() == 404 {
            // If the script doesn't exist, we can't get a version
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status().to_string();
            let text = resp.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        // Parse JSON for the list of versions
        let list_data: CfListVersionsResponse = resp.json().await?;
        if !list_data.success {
            // The request returned 200 but CF said "success = false"
            return Ok(None);
        }

        // If there are no versions at all, we have nothing to examine
        if list_data.result.items.is_empty() {
            return Ok(None);
        }

        // For simplicity, let's assume the first in `result` is the *newest* version.
        // If the API is unsorted, you might need to pick by `created_on` or similar.
        let latest_version_id = &list_data.result.items[0].id;

        // 2) Get details for that specific version
        let version_info_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/workers/scripts/{}/versions/{}",
            self.account_id, script_name, latest_version_id
        );

        let resp2 = self
            .client
            .get(&version_info_url)
            .headers(self.api_auth.headers())
            .send()
            .await?;

        if !resp2.status().is_success() {
            let status = resp2.status().to_string();
            let text = resp2.text().await?;
            return Err(DeployError::UnexpectedResponse(format!(
                "{}: {}",
                status, text
            )));
        }

        let version_data: CfSingleVersionResponse = resp2.json().await?;
        if !version_data.success {
            return Ok(None);
        }

        // 3) Look for our plaintext binding
        let binding = version_data
            .result
            .resources
            .bindings
            .iter()
            .find(|b| b.name == WORKER_VERSION_TAG && b.type_ == "plain_text");

        if let Some(found) = binding {
            // Return the text if present
            if let Some(text_value) = &found.text {
                return Ok(Some(text_value.clone()));
            }
        }

        // Otherwise, we didn't find the binding or there's no text
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
        let mut bindings_json: Vec<serde_json::Value> = metadata
            .bindings
            .iter()
            .map(|b| {
                json!({
                    "type": b.type_,
                    "name": b.name,
                    "namespace_id": b.namespace_id,
                })
            })
            .collect();

        let tag_binding = json!({
            "type": "plain_text",
            "name": WORKER_VERSION_TAG,
            "text": metadata.tag,
        });

        bindings_json.push(tag_binding);

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
                    .mime_str(&part.content_type)
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
        let result_data: CloudflareApiResponse = resp.json().await?;

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

        let result_data: CloudflareApiResponse = resp.json().await?;
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

        let result_data: CloudflareApiResponse = resp.json().await?;
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

        let result_data: CloudflareApiResponse = resp.json().await?;
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

        let result_data: CloudflareApiResponse = resp.json().await?;
        if !result_data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn get_ruleset(
        &self,
        zone_id: String,
        name: String,
        phase: String,
    ) -> Result<Option<String>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/rulesets",
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

        let data: RulesetListResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(rulesets) = data.result {
            for rs in rulesets {
                if rs.phase == phase && rs.name == name {
                    return Ok(Some(rs.id));
                }
            }

            Ok(None)
        } else {
            Ok(None)
        }
    }

    async fn create_ruleset(
        &self,
        zone_id: String,
        name: String,
        phase: String,
    ) -> Result<String, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/rulesets",
            zone_id
        );

        let body = serde_json::json!({
            "name": name,
            "kind": "zone",
            "phase": phase,
            "rules": [],
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

        let data: RulesetResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(ruleset) = data.result {
            Ok(ruleset.id)
        } else {
            Err(DeployError::UnexpectedResponse(
                "No ruleset ID returned".to_string(),
            ))
        }
    }

    async fn update_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
        rules: Vec<Rule>,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/rulesets/{}",
            zone_id, ruleset_id
        );

        let body = serde_json::json!({
            "rules": rules
        });

        let resp = self
            .client
            .put(&url)
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

        let data: RulesetResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn remove_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
    ) -> Result<(), DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/rulesets/{}",
            zone_id, ruleset_id
        );

        let body = serde_json::json!({
            "rules": []
        });

        let resp = self
            .client
            .put(&url)
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

        let data: RulesetResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        Ok(())
    }

    async fn get_ruleset_rules(
        &self,
        zone_id: String,
        ruleset_id: String,
    ) -> Result<Vec<Rule>, DeployError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/rulesets/{}",
            zone_id, ruleset_id
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

        let data: RulesetResponse = resp.json().await?;
        if !data.success {
            return Err(DeployError::OtherError);
        }

        if let Some(ruleset) = data.result {
            if let Some(rules) = ruleset.rules {
                Ok(rules)
            } else {
                Ok(vec![])
            }
        } else {
            Ok(vec![])
        }
    }
}
