use colored::Colorize;

use crate::{
    Result,
    services::local_server,
    session::{list_session_rows, print_sessions_table},
};

#[derive(clap::Args)]
pub struct Args {}

pub async fn run(_args: &Args) -> Result<()> {
    if !local_server::is_reachable().await {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    print_sessions_table(&list_session_rows().await, None);

    Ok(())
}
