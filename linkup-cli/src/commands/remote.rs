use crate::{
    local_config::{upload_state, LocalState, ServiceTarget},
    services::{self, find_service_pid, BackgroundService},
    Result,
};

use anyhow::anyhow;
use colored::Colorize;

#[derive(clap::Args)]
pub struct Args {
    service_names: Vec<String>,

    #[arg(
        short,
        long,
        help = "Route all the services to remote. Cannot be used with SERVICE_NAMES.",
        conflicts_with = "service_names"
    )]
    all: bool,
}

pub async fn remote(args: &Args) -> Result<()> {
    if args.service_names.is_empty() && !args.all {
        return Err(anyhow!("No service names provided"));
    }

    if !LocalState::exists() {
        println!(
            "{}",
            "Seems like you don't have any state yet to point to remote.".yellow()
        );
        println!("{}", "Have you run `linkup start` at least once?".yellow());

        return Ok(());
    }

    let mut state = LocalState::load()?;

    if find_service_pid(services::LocalServer::ID).is_none() {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run `linkup start` first."
                .yellow()
        );

        return Ok(());
    }

    if args.all {
        for service in state.services.iter_mut() {
            service.current = ServiceTarget::Remote;
        }
    } else {
        for service_name in &args.service_names {
            let service = state
                .services
                .iter_mut()
                .find(|s| s.name.as_str() == service_name)
                .ok_or_else(|| anyhow!("Service with name '{}' does not exist", service_name))?;

            service.current = ServiceTarget::Remote;
        }
    }

    state.save()?;
    upload_state(&state).await?;

    if args.all {
        println!("Linkup is routing all traffic to the remote servers");
    } else {
        println!(
            "Linkup is routing {} traffic to the remote server",
            args.service_names.join(", ")
        );
    }

    Ok(())
}
