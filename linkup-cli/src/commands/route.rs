use anyhow::{Context, anyhow};
use colored::Colorize;
use url::Url;

use crate::{
    Result, services,
    state::{ServiceTarget, State},
};

#[derive(clap::ValueEnum, Clone)]
pub enum TargetArg {
    Local,
    Remote,
}

#[derive(clap::Args)]
pub struct Args {
    target: TargetArg,

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
        TargetArg::Local => ServiceTarget::Local,
        TargetArg::Remote => ServiceTarget::Remote,
    };

    let (state, target_map) = if let Some(session_name) = &args.session {
        let mut state = State::load_with_suffix(session_name)
            .with_context(|| format!("Failed to load state for session '{}'", session_name))?;

        let target_map =
            set_service_targets(&mut state, &args.service_names, args.all, service_target)?;

        services::local_server::update_isolated_state(&mut state).await?;
        state.save_with_suffix(session_name)?;

        (state, target_map)
    } else {
        let mut state = State::load()?;

        let target_map =
            set_service_targets(&mut state, &args.service_names, args.all, service_target)?;

        services::local_server::update_state(&mut state).await?;

        (state, target_map)
    };

    let name_width = target_map
        .iter()
        .map(|(service_name, _)| service_name.len())
        .max()
        .unwrap_or(0);

    println!("\nSession: {}", state.linkup.session_name.bold());
    for (service_name, url) in &target_map {
        println!(
            "  {:<width$}  ->  {}",
            service_name,
            url,
            width = name_width,
        );
    }

    Ok(())
}

fn set_service_targets(
    state: &mut State,
    service_names: &[String],
    all: bool,
    target: ServiceTarget,
) -> Result<Vec<(String, Url)>> {
    let mut new_targets = Vec::new();

    if all {
        for service in state.services.iter_mut() {
            service.current = target.clone();

            new_targets.push((service.config.name.clone(), service.current_url()));
        }
    } else {
        for service_name in service_names {
            let service = state
                .services
                .iter_mut()
                .find(|s| s.config.name.as_str() == service_name)
                .ok_or_else(|| anyhow!("Service '{}' does not exist", service_name))?;

            service.current = target.clone();

            new_targets.push((service.config.name.clone(), service.current_url()));
        }
    }

    new_targets.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(new_targets)
}
