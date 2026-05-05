use colored::Colorize;

use crate::{
    Result,
    services::local_server,
    session::{list_session_rows, print_sessions_table},
};

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub json: bool,
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

    let sessions_rows = list_session_rows().await;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&sessions_rows)
                .expect("Failed to serialize sessions rows")
        );
    } else {
        print_sessions_table(&sessions_rows, None);
    }

    Ok(())
}
