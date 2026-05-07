use std::path::Path;

use anyhow::Context;
use colored::Colorize;

use linkup::{NameKind, Session, SessionKind, UpsertSessionRequest};
use linkup_clients::LocalServerClient;

use crate::{
    Result, commands,
    services::local_server,
    session::{SessionRow, print_sessions_table},
    state::State,
};

#[derive(clap::Args)]
pub(super) struct Args {
    #[arg(help = "Optional name for the isolated session")]
    pub name: Option<String>,
}

pub(super) async fn run(args: &Args, config_arg: Option<&Path>) -> Result<()> {
    if !local_server::is_reachable().await {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    if !commands::local_dns::is_installed(None, config_arg) {
        println!(
            "{}",
            "Isolated sessions requires Local DNS to be configured.\nPlease run 'linkup local-dns install' first."
                .yellow()
        );

        return Ok(());
    }

    let mut isolated_state = State::from_config(config_arg)?;
    let session: Session = (&isolated_state).into();

    let upsert_request = match &args.name {
        Some(name) => UpsertSessionRequest::Named {
            desired_name: name.clone(),
            session_token: session.session_token,
            services: session.services,
            domains: session.domains,
            cache_routes: session.cache_routes,
        },
        None => UpsertSessionRequest::Unnamed {
            name_kind: NameKind::SixChar,
            session_token: Some(session.session_token),
            services: session.services,
            domains: session.domains,
            cache_routes: session.cache_routes,
        },
    };

    let local_server_client = LocalServerClient::new(&local_server::url());

    let response = local_server_client
        .isolated_session(&upsert_request)
        .await
        .context("Failed to create isolated session")?;

    isolated_state.linkup.session_name = response.session_name.clone();
    isolated_state.save_with_suffix(&response.session_name)?;

    print_sessions_table(
        &[SessionRow::from_state(
            &isolated_state,
            SessionKind::Isolated,
        )],
        None,
    );

    Ok(())
}
