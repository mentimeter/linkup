use crate::local_config::{config_path, get_config};
use crate::status::{format_state_domains, print_session_status, SessionStatus};
use crate::CliError;
use linkup::CreatePreviewRequest;
use reqwest::blocking::Client;
use reqwest::StatusCode;

pub fn preview(
    config: &Option<String>,
    services: &[(String, String)],
    print_request: bool,
) -> Result<(), CliError> {
    if services.is_empty() {
        // TODO: Oliver don't care about this error handling (type)
        return Err(CliError::BadConfig("No services specified".to_string()));
    }

    let config_path = config_path(config)?;
    let input_config = get_config(&config_path)?;
    let create_preview_request: CreatePreviewRequest =
        input_config.create_preview_request(services);
    let url = input_config.linkup.remote.clone();
    let create_req_json = serde_json::to_string(&create_preview_request)
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    if print_request {
        println!("{}", create_req_json);
        return Ok(());
    }

    let client = Client::new();
    let endpoint = url
        .join("/preview")
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

    print_session_status(SessionStatus {
        name: preview_name.clone(),
        domains: format_state_domains(&preview_name, &input_config.domains),
    });

    Ok(())
}

pub fn parse_services_tuples(arg: &str) -> std::result::Result<(String, String), String> {
    let (k, v) = arg
        .split_once('=')
        .ok_or_else(|| "Service tuple must be of the form <service>=<url>".to_string())?;

    Ok((k.to_string(), v.to_string()))
}
