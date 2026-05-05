use anyhow::Context;
use colored::Colorize;

use linkup_clients::LocalServerClient;

use crate::{Result, services::local_server, state::State};

#[derive(clap::Args)]
pub struct Args {
    #[arg(help = "Name of the isolated session to delete")]
    pub name: String,
}

pub async fn run(args: &Args) -> Result<()> {
    if !local_server::is_reachable().await {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    let main_state = State::load().context("Failed to load local state")?;
    if main_state.linkup.session_name == args.name {
        println!(
            "{}",
            "Cannot delete the main session. Run 'linkup stop' to stop Linkup entirely.".yellow()
        );

        return Ok(());
    }

    let client = LocalServerClient::new(&local_server::url());
    client
        .delete_session(&args.name)
        .await
        .with_context(|| format!("Failed to delete session '{}'", args.name))?;

    if let Err(e) = State::delete_with_suffix(&args.name) {
        log::warn!("Session deleted from server but failed to remove state file: {e}");
    }

    println!("Session '{}' deleted.", args.name);

    Ok(())
}
