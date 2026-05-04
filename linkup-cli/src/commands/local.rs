use anyhow::{Context, anyhow};
use colored::Colorize;

use crate::{
    Result, services,
    state::{ServiceTarget, State},
};

#[derive(clap::Args)]
pub struct Args {
    service_names: Vec<String>,

    #[arg(
        short,
        long,
        help = "Route all the services to local. Cannot be used with SERVICE_NAMES.",
        conflicts_with = "service_names"
    )]
    all: bool,

    #[arg(
        long,
        value_name = "NAME",
        help = "Session to update (defaults to the tunneled session)"
    )]
    session: Option<String>,
}

pub async fn local(args: &Args) -> Result<()> {
    if args.service_names.is_empty() && !args.all {
        return Err(anyhow!("No service names provided"));
    }

    if !State::exists() {
        println!(
            "{}",
            "Seems like you don't have any state yet to point to local.".yellow()
        );
        println!("{}", "Have you run 'linkup start' at least once?".yellow());

        return Ok(());
    }

    if services::local_server::find_pid().is_none() {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    if let Some(session_name) = &args.session {
        let mut state = State::load_with_suffix(session_name)
            .with_context(|| format!("Failed to load state for session '{}'", session_name))?;

        set_service_targets(
            &mut state,
            &args.service_names,
            args.all,
            ServiceTarget::Local,
        )?;

        services::local_server::update_isolated_state(&mut state).await?;
        state.save_with_suffix(session_name)?;
    } else {
        let mut state = State::load()?;

        set_service_targets(
            &mut state,
            &args.service_names,
            args.all,
            ServiceTarget::Local,
        )?;

        services::local_server::update_state(&mut state).await?;
    }

    if args.all {
        println!("Linkup is routing all traffic to the local servers");
    } else {
        println!(
            "Linkup is routing {} traffic to the local server",
            args.service_names.join(", ")
        );
    }

    Ok(())
}

fn set_service_targets(
    state: &mut State,
    service_names: &[String],
    all: bool,
    target: ServiceTarget,
) -> Result<()> {
    if all {
        for service in state.services.iter_mut() {
            service.current = target.clone();
        }
    } else {
        for service_name in service_names {
            let service = state
                .services
                .iter_mut()
                .find(|s| s.config.name.as_str() == service_name)
                .ok_or_else(|| anyhow!("Service '{}' does not exist", service_name))?;

            service.current = target.clone();
        }
    }

    Ok(())
}
