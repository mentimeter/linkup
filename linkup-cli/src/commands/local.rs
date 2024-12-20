use crate::{
    local_config::{upload_state, LocalState, ServiceTarget},
    CliError,
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
}

pub async fn local(args: &Args) -> Result<(), CliError> {
    if args.service_names.is_empty() && !args.all {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = LocalState::load()?;

    if args.all {
        for service in state.services.iter_mut() {
            service.current = ServiceTarget::Local;
        }
    } else {
        for service_name in &args.service_names {
            let service = state
                .services
                .iter_mut()
                .find(|s| s.name.as_str() == service_name)
                .ok_or_else(|| CliError::NoSuchService(service_name.to_string()))?;
            service.current = ServiceTarget::Local;
        }
    }

    state.save()?;
    upload_state(&state).await?;

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
