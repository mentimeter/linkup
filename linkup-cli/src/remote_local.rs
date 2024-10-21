use url::Url;

use crate::{
    background_booting::{load_config, ServerConfig},
    local_config::{LocalState, ServiceTarget},
    CliError, LINKUP_LOCALSERVER_PORT,
};

pub fn remote(service_names: &[String], all: bool) -> Result<(), CliError> {
    if service_names.is_empty() && !all {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = LocalState::load()?;

    for service in state.services.iter_mut() {
        if all {
            service.current = ServiceTarget::Remote;
            continue;
        }

        if service_names.contains(&service.name) {
            service.current = ServiceTarget::Remote;
        } else {
            return Err(CliError::NoSuchService(service.name.clone()));
        }
    }

    state.save()?;
    load_server_states(state)?;

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

pub fn local(service_names: &[String], all: bool) -> Result<(), CliError> {
    if service_names.is_empty() && !all {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = LocalState::load()?;

    for service in state.services.iter_mut() {
        if all {
            service.current = ServiceTarget::Local;
            continue;
        }

        if service_names.contains(&service.name) {
            service.current = ServiceTarget::Local;
        } else {
            return Err(CliError::NoSuchService(service.name.clone()));
        }
    }

    state.save()?;
    load_server_states(state)?;

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

fn load_server_states(state: LocalState) -> Result<(), CliError> {
    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        .expect("linkup url invalid");

    let server_config = ServerConfig::from(&state);

    let _ = load_config(
        &state.linkup.remote,
        &state.linkup.session_name.clone(),
        server_config.remote,
    )?;
    let _ = load_config(&local_url, &state.linkup.session_name, server_config.local)?;

    Ok(())
}
