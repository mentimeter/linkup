use crate::{
    local_config::{upload_state, LocalState, ServiceTarget},
    CliError,
};

pub async fn remote(service_names: &[String], all: bool) -> Result<(), CliError> {
    if service_names.is_empty() && !all {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = LocalState::load()?;

    if all {
        for service in state.services.iter_mut() {
            service.current = ServiceTarget::Remote;
        }
    } else {
        for service_name in service_names {
            let service = state
                .services
                .iter_mut()
                .find(|s| s.name.as_str() == service_name)
                .ok_or_else(|| CliError::NoSuchService(service_name.to_string()))?;
            service.current = ServiceTarget::Remote;
        }
    }

    state.save()?;
    upload_state(&state).await?;

    if all {
        println!("Linkup is routing all traffic to the remote servers");
    } else {
        println!(
            "Linkup is routing {} traffic to the remote server",
            service_names.join(", ")
        );
    }

    Ok(())
}

pub async fn local(service_names: &[String], all: bool) -> Result<(), CliError> {
    if service_names.is_empty() && !all {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = LocalState::load()?;

    if all {
        for service in state.services.iter_mut() {
            service.current = ServiceTarget::Local;
        }
    } else {
        for service_name in service_names {
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

    if all {
        println!("Linkup is routing all traffic to the local servers");
    } else {
        println!(
            "Linkup is routing {} traffic to the local server",
            service_names.join(", ")
        );
    }

    Ok(())
}
