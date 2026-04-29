use anyhow::Context;
use linkup::{Session, UpsertSessionRequest};
use linkup_clients::LocalServerClient;

use crate::services;
use crate::session::{SessionStatus, format_state_domains};
use crate::{Result, state};

#[derive(clap::Args)]
pub struct Args {
    #[arg(help = "Name for the isolated session")]
    pub name: String,
}

pub async fn fork(args: &Args, config_path: &Option<String>) -> Result<()> {
    let config_path = state::config_path(config_path)?;
    let config = state::get_config(&config_path)?;
    let state = state::config_to_state(config, config_path);
    let session: Session = (&state).into();

    let upsert_request = UpsertSessionRequest::Named {
        desired_name: args.name.clone(),
        session_token: session.session_token,
        services: session.services,
        domains: session.domains.clone(),
        cache_routes: session.cache_routes,
    };

    let local_server_client = LocalServerClient::new(&services::local_server::url());

    let response = local_server_client
        .isolated_session(&upsert_request)
        .await
        .context("Failed to create isolated session")?;

    let domains = format_state_domains(&response.session_name, &state.domains);

    SessionStatus {
        name: response.session_name,
        domains,
    }
    .print();

    Ok(())
}
