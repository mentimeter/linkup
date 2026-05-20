mod create_preview;

use std::path::Path;

use clap::Subcommand;

use crate::Result;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[clap(about = "Create a preview session")]
    CreatePreview(create_preview::Args),
}

pub async fn sessions(args: &Args, config: Option<&Path>) -> Result<()> {
    match &args.command {
        Command::CreatePreview(args) => create_preview::run(args, config).await,
    }
}
