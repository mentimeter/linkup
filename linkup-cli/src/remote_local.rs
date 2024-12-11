use url::Url;

use crate::{
    background_booting::{load_config, ServerConfig},
    local_config::{LocalState, ServiceTarget},
    CliError, LINKUP_LOCALSERVER_PORT,
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
    load_server_states(state).await?;

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
    load_server_states(state).await?;

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

async fn load_server_states(state: LocalState) -> Result<(), CliError> {
    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        .expect("linkup url invalid");

    let server_config = ServerConfig::from(&state);

    let _ = load_config(
        &state.linkup.remote,
        &state.linkup.session_name.clone(),
        server_config.remote,
    )
    .await?;
    let _ = load_config(&local_url, &state.linkup.session_name, server_config.local).await?;

    Ok(())
}
