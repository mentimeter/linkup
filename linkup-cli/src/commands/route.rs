use anyhow::{Context, anyhow};
use colored::Colorize;

use crate::{
    Result, services,
    state::{ServiceTarget, State},
};

#[derive(clap::ValueEnum, Clone)]
pub enum Target {
    Local,
    Remote,
}

#[derive(clap::Args)]
pub struct Args {
    target: Target,

    #[arg(required_unless_present = "all")]
    service_names: Vec<String>,

    #[arg(
        short,
        long,
        help = "Route all services. Cannot be used with SERVICE_NAMES.",
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

pub async fn route(args: &Args) -> Result<()> {
    if !services::local_server::is_reachable().await {
        println!(
            "{}",
            "Seems like your local Linkup server is not running. Please run 'linkup start' first."
                .yellow()
        );

        return Ok(());
    }

    let service_target = match args.target {
        Target::Local => ServiceTarget::Local,
        Target::Remote => ServiceTarget::Remote,
    };

    if let Some(session_name) = &args.session {
        let mut state = State::load_with_suffix(session_name)
            .with_context(|| format!("Failed to load state for session '{}'", session_name))?;

        set_service_targets(&mut state, &args.service_names, args.all, service_target)?;

        services::local_server::update_isolated_state(&mut state).await?;
        state.save_with_suffix(session_name)?;
    } else {
        let mut state = State::load()?;

        set_service_targets(&mut state, &args.service_names, args.all, service_target)?;

        services::local_server::update_state(&mut state).await?;
    }

    let target_label = match args.target {
        Target::Local => "local",
        Target::Remote => "remote",
    };

    if args.all {
        println!(
            "Linkup is routing all traffic to the {} servers",
            target_label
        );
    } else {
        println!(
            "Linkup is routing {} traffic to the {} server",
            args.service_names.join(", "),
            target_label
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
