mod cloudflare;
mod deploy;
mod destroy;
mod notifier;

use anyhow::Result;
use clap::Subcommand;

#[derive(clap::Args)]
pub struct Args {
    #[arg(short = 'e', long = "email", help = "Cloudflare user email")]
    pub email: String,

    #[arg(short = 'k', long = "api-key", help = "Cloudflare user global API Key")]
    pub api_key: String,

    #[arg(short = 'a', long = "account-id", help = "Cloudflare account ID")]
    pub account_id: String,

    #[arg(
        short = 'z',
        long = "zone-ids",
        help = "Cloudflare zone IDs",
        num_args = 1..,
        required = true
    )]
    pub zone_ids: Vec<String>,

    #[clap(subcommand)]
    subcommand: InfraSubcommand,
}

#[derive(Subcommand)]
pub enum InfraSubcommand {
    #[clap(about = "Deploy services to Cloudflare")]
    Deploy(deploy::DeployArgs),

    #[clap(about = "Destroy/remove linkup installation from Cloudflare")]
    Destroy(destroy::DestroyArgs),
}

pub async fn infra(args: &Args) -> Result<()> {
    match &args.subcommand {
        InfraSubcommand::Deploy(deploy_args) => deploy::deploy(deploy_args, args).await,
        InfraSubcommand::Destroy(destroy_args) => destroy::destroy(destroy_args, args).await,
    }
}
