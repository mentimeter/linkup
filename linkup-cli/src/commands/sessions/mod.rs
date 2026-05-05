mod create_isolated;
mod create_preview;
mod delete;
mod list;

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

    #[clap(about = "Create an isolated session")]
    CreateIsolated(create_isolated::Args),

    #[clap(about = "Delete session")]
    Delete(delete::Args),

    #[clap(about = "List sessions", aliases = ["ls"])]
    List(list::Args),
}

pub async fn sessions(args: &Args, config: &Option<String>) -> Result<()> {
    match &args.command {
        Command::CreatePreview(args) => create_preview::run(args, config).await,
        Command::CreateIsolated(args) => create_isolated::run(args, config).await,
        Command::Delete(args) => delete::run(args).await,
        Command::List(args) => list::run(args).await,
    }
}
