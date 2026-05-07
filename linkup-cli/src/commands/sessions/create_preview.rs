use std::path::Path;

use anyhow::Context;
use clap::builder::ValueParser;
use url::Url;

use linkup::SessionKind;
use linkup_clients::WorkerClient;

use crate::{
    Result,
    config::load_config_with_override,
    session::{SessionRow, format_state_domains, print_sessions_table},
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(help = "Optional name for the preview session")]
    pub name: Option<String>,

    #[arg(
        help = "<service>=<url> pairs to override.",
        value_parser = ValueParser::new(parse_services_tuple),
        num_args = 0..,
    )]
    pub services: Vec<(String, Url)>,

    #[arg(long, help = "Print the request body instead of sending it.")]
    pub print_request: bool,
}

pub async fn run(args: &Args, config_arg: Option<&Path>) -> Result<()> {
    let (config, _) = load_config_with_override(config_arg)?;

    let upsert_session_request =
        linkup::create_preview_req_from_config(&config, args.name.clone(), &args.services);

    if args.print_request {
        let create_req_json = serde_json::to_string(&upsert_session_request)
            .context("Failed to encode request to JSON string")?;

        println!("{}", create_req_json);

        return Ok(());
    }

    let worker_client = WorkerClient::new(&config.linkup.worker_url, &config.linkup.worker_token);

    let preview_session = worker_client
        .preview_session(&upsert_session_request)
        .await?;

    let preview_name = preview_session.session_name;

    print_sessions_table(
        &[SessionRow {
            domains: format_state_domains(&preview_name, &config.domains),
            name: preview_name,
            kind: SessionKind::Preview,
        }],
        None,
    );

    Ok(())
}

fn parse_services_tuple(arg: &str) -> std::result::Result<(String, Url), String> {
    let (k, v) = arg
        .split_once('=')
        .ok_or_else(|| "Service tuple must be of the form <service>=<url>".to_string())?;

    let url = Url::parse(v).map_err(|e| format!("Invalid URL '{v}': {e}"))?;

    Ok((k.to_string(), url))
}
