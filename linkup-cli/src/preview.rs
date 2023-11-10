use reqwest::blocking::Client;
use reqwest::StatusCode;
use linkup::CreatePreviewRequest;
use crate::CliError;
use crate::local_config::{config_path, get_config};

pub fn preview(config: &Option<String>, services: &[String]) -> Result<(), CliError> {
    let services: Vec<(String, String)> = services
        .iter()
        .filter_map(|item| item.split_once('=') )
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let config_path = config_path(config)?;
    let input_config = get_config(&config_path)?;
    let create_preview_request: CreatePreviewRequest = input_config.create_preview_request(&services);

    let client = Client::new();
    let url = input_config.linkup.remote.clone();
    let endpoint = url
        .join("/preview")
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let create_req_json = serde_json::to_string(&create_preview_request)
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let response = client
        .post(endpoint.clone())
        .body(create_req_json)
        .send()
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let preview_name = match response.status() {
        StatusCode::OK => {
            let content = response
                .text()
                .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;
            Ok(content)
        }
        _ => Err(CliError::LoadConfig(
            endpoint.to_string(),
            format!("status code: {}", response.status()),
        )),
    }?;

    println!("Preview name: {}", preview_name);

    Ok(())
}