use crate::commands::status::{format_state_domains, SessionStatus};
use crate::local_config::{config_path, get_config};
use crate::worker_client::WorkerClient;
use crate::CliError;
use clap::builder::ValueParser;
use linkup::CreatePreviewRequest;

#[derive(clap::Args)]
pub struct Args {
    #[arg(
        help = "<service>=<url> pairs to preview.",
        value_parser = ValueParser::new(parse_services_tuple),
        required = true,
        num_args = 1..,
    )]
    services: Vec<(String, String)>,

    #[arg(long, help = "Print the request body instead of sending it.")]
    print_request: bool,
}

pub async fn preview(args: &Args, config: &Option<String>) -> Result<(), CliError> {
    let config_path = config_path(config)?;
    let input_config = get_config(&config_path)?;
    let create_preview_request: CreatePreviewRequest =
        input_config.create_preview_request(&args.services);
    let url = input_config.linkup.remote.clone();
    let create_req_json = serde_json::to_string(&create_preview_request)
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    if args.print_request {
        println!("{}", create_req_json);
        return Ok(());
    }

    let preview_name = WorkerClient::from(&input_config)
        .preview(&create_preview_request)
        .await
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

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
