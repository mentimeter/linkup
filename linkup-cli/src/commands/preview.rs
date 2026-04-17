use crate::Result;
use crate::commands::status::{SessionStatus, format_state_domains};
use crate::state::{config_path, get_config};
use anyhow::Context;
use clap::builder::ValueParser;
use linkup::UpsertSessionRequest;
use linkup_clients::WorkerClient;
use url::Url;

#[derive(clap::Args)]
pub struct Args {
    #[arg(
        help = "<service>=<url> pairs to preview.",
        value_parser = ValueParser::new(parse_services_tuple),
        required = true,
        num_args = 1..,
    )]
    services: Vec<(String, Url)>,

    #[arg(long, help = "Print the request body instead of sending it.")]
    print_request: bool,
}

pub async fn preview(args: &Args, config: &Option<String>) -> Result<()> {
    let config_path = config_path(config)?;
    let input_config = get_config(&config_path)?;
    let upsert_session_request: UpsertSessionRequest =
        linkup::create_preview_req_from_config(&input_config, &args.services);
    let url = input_config.linkup.worker_url.clone();

    if args.print_request {
        let create_req_json = serde_json::to_string(&upsert_session_request)
            .context("Failed to encode request to JSON string")?;

        println!("{}", create_req_json);

        return Ok(());
    }

    let preview_name = WorkerClient::new(&url, &input_config.linkup.worker_token)
        .preview_session(&upsert_session_request)
        .await
        .with_context(|| format!("Failed to send preview request to {}", url))?;

    let status = SessionStatus {
        name: preview_name.clone(),
        domains: format_state_domains(&preview_name, &input_config.domains),
    };

    status.print();

    Ok(())
}

pub fn parse_services_tuple(arg: &str) -> std::result::Result<(String, String), String> {
    let (k, v) = arg
        .split_once('=')
        .ok_or_else(|| "Service tuple must be of the form <service>=<url>".to_string())?;

    Ok((k.to_string(), v.to_string()))
}
