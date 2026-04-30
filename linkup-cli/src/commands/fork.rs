use anyhow::Context;
use linkup::{Session, UpsertSessionRequest};
use linkup_clients::LocalServerClient;

use crate::Result;
use crate::services;
use crate::session::{SessionRow, print_sessions_table};
use crate::state;

#[derive(clap::Args)]
pub struct Args {
    #[arg(help = "Name for the isolated session")]
    pub name: String,
}

pub async fn fork(args: &Args, config_path: &Option<String>) -> Result<()> {
    let config_path = state::config_path(config_path)?;
    let config = state::get_config(&config_path)?;
    let mut state = state::config_to_state(config, config_path);
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

    state.linkup.session_name = response.session_name.clone();
    state.save_with_suffix(&response.session_name)?;

    print_sessions_table(
        &[SessionRow::from_state(&state, "isolated".to_string())],
        None,
    );

    Ok(())
}
