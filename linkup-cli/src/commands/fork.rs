use anyhow::Context;
use linkup::{Session, UpsertSessionRequest};
use linkup_clients::LocalServerClient;

use crate::Result;
use crate::session::{SessionStatus, format_state_domains};
use crate::services;
use crate::state::State;

#[derive(clap::Args)]
pub struct Args {
    #[arg(help = "Name for the isolated session")]
    pub name: String,
}

pub async fn fork(args: &Args) -> Result<()> {
    let state = State::load().context("Failed to load local state")?;

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
